use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use hyper::body::Buf;
use hyper::service::{make_service_fn, service_fn};
use hyper::upgrade::Upgraded;
use hyper::{body, header, upgrade, Client, StatusCode, Uri};
use hyper::{server::conn::AddrStream, Body, Request, Response, Server};
use lazy_static::lazy_static;
use route_recognizer::Router;
use serde_json::{json, Value};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::convert::{Infallible, TryInto};
use std::fs;
use std::io::Read;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use tokio::sync::Mutex;
use tokio_tungstenite::WebSocketStream;
use tungstenite::{handshake, Error, Message};
use urlencoding::decode;

// Compares two strings the way that Windows Explorer does - when both
// filenames have one or more digits in the same location, consider
// those digits to be part of a single token and compare them numerically
// e.g. "1-456" comes after "1-7" even though 4 comes before 7, since
// 456 is larger than 7.
fn smart_cmp(a: &str, b: &str) -> Ordering {
    if a == b {
        return Ordering::Equal;
    }

    let mut a_chars = a.chars().peekable();
    let mut b_chars = b.chars().peekable();
    loop {
        if let Some(a_next) = a_chars.next() {
            if let Some(b_next) = b_chars.next() {
                if char::is_numeric(a_next) && char::is_numeric(b_next) {
                    let mut a_num = (a_next as u8 - b'0') as i32;
                    let mut b_num = (b_next as u8 - b'0') as i32;
                    while let Some(&a_next) = a_chars.peek() {
                        if char::is_numeric(a_next) {
                            a_num *= 10;
                            a_num += (a_next as u8 - b'0') as i32;
                            a_chars.next();
                        } else {
                            break;
                        }
                    }
                    while let Some(&b_next) = b_chars.peek() {
                        if char::is_numeric(b_next) {
                            b_num *= 10;
                            b_num += (b_next as u8 - b'0') as i32;
                            b_chars.next();
                        } else {
                            break;
                        }
                    }
                    if a_num.cmp(&b_num) != Ordering::Equal {
                        return a_num.cmp(&b_num);
                    }
                } else {
                    if a_next.cmp(&b_next) != Ordering::Equal {
                        return a_next.cmp(&b_next);
                    }
                }
            } else {
                return Ordering::Greater;
            }
        } else {
            return Ordering::Less;
        }
    }
}

#[derive(Debug)]
struct Config {
    port: u16,
    img_folder: PathBuf,
    mirror: Option<String>,
}

fn init_config() -> Config {
    let config_file: serde_json::Value =
        serde_json::from_str(&fs::read_to_string("config.json").unwrap_or("{}".to_string()))
            .unwrap();
    let mut config = Config {
        port: 30000,
        img_folder: PathBuf::from_str(".").unwrap(),
        mirror: None,
    };
    let mut args = pico_args::Arguments::from_env();

    if let Ok(folder) = args.value_from_str("--folder") {
        config.img_folder = folder;
    } else if let Some(serde_json::Value::String(folder)) = config_file.get("folder") {
        config.img_folder = PathBuf::from_str(&folder).unwrap();
    }

    if let Ok(port) = args.value_from_str("--port") {
        config.port = port;
    } else if let Some(serde_json::Value::Number(port)) = config_file.get("port") {
        // * gazes longlingly at https://github.com/rust-lang/rust/issues/31436 *
        if let Some(port) = port.as_u64() {
            if let Ok(port) = port.try_into() {
                config.port = port;
            }
        }
    }

    if let Ok(mirror) = args.value_from_str("--mirror") {
        config.mirror = Some(mirror);
    } else if let Some(serde_json::Value::String(mirror)) = config_file.get("mirror") {
        config.mirror = Some(mirror.clone());
    }

    config
}

#[derive(Default)]
struct Status {
    connections: Vec<(SocketAddr, SplitSink<WebSocketStream<Upgraded>, Message>)>,
    current_pages: HashMap<String, String>,
}

lazy_static! {
    static ref STATUS: Mutex<Status> = Mutex::new(Default::default());
    static ref CONFIG: Config = init_config();
}

fn err(code: u16) -> Response<Body> {
    Response::builder()
        .status(code)
        .body(Body::from(code.to_string()))
        .unwrap()
}

async fn handle_request(
    request: Request<Body>,
    remote_addr: SocketAddr,
) -> Result<Response<Body>, Infallible> {
    match (
        request.uri().path(),
        request.headers().contains_key(header::UPGRADE),
    ) {
        ("/msg", true) => handle_ws_request(request, remote_addr).await,
        ("/msg", false) => {
            //handle the case where the url is /msg, but does not have an Upgrade field
            Ok(Response::new(Body::from(
                "Try connecting to this endpoint with a websocket.",
            )))
        }
        (url @ _, false) => Ok(handle_static_request(url).await.unwrap_or_else(|e| e)),
        (_, true) => {
            //handle any other url with an Upgrade header field
            Ok(Response::new(Body::from(
                "Websockets should connect to /msg.",
            )))
        }
    }
}

async fn handle_ws_request(
    mut request: Request<Body>,
    remote_addr: SocketAddr,
) -> Result<Response<Body>, Infallible> {
    //assume request is a handshake, so create the handshake response
    let response = match handshake::server::create_response_with_body(&request, || Body::empty()) {
        Ok(response) => {
            //in case the handshake response creation succeeds,
            //spawn a task to handle the websocket connection
            tokio::spawn(async move {
                //using the hyper feature of upgrading a connection
                match upgrade::on(&mut request).await {
                    //if successfully upgraded
                    Ok(upgraded) => {
                        //create a websocket stream from the upgraded object
                        let ws_stream = WebSocketStream::from_raw_socket(
                            //pass the upgraded object
                            //as the base layer stream of the Websocket
                            upgraded,
                            tokio_tungstenite::tungstenite::protocol::Role::Server,
                            None,
                        )
                        .await;

                        //we can split the stream into a sink and a stream
                        let (ws_write, mut ws_read) = ws_stream.split();

                        STATUS
                            .lock()
                            .await
                            .connections
                            .push((remote_addr, ws_write));
                        println!("Connected to {:?}", remote_addr);

                        loop {
                            match ws_read.next().await {
                                Some(Ok(Message::Close(_)))
                                | Some(Err(Error::ConnectionClosed))
                                | Some(Err(Error::Io(_)))
                                | None => break,
                                Some(Ok(Message::Text(msg))) => {
                                    let mut status = STATUS.lock().await;
                                    let mut i = status.connections.len() as isize - 1;
                                    let msg_json: Value = serde_json::from_str(&msg).unwrap();
                                    let comic = msg_json.get("comic").unwrap().as_str().unwrap();
                                    if let Some(pg) = msg_json.get("page") {
                                        *status
                                            .current_pages
                                            .entry(comic.to_string())
                                            .or_default() = pg.as_str().unwrap().to_string();
                                        while i >= 0 {
                                            let (addr, conn) = &mut status.connections[i as usize];
                                            if *addr != remote_addr {
                                                match conn.send(Message::Text(msg.clone())).await {
                                                    Ok(()) => {}
                                                    Err(Error::ConnectionClosed | Error::Io(_)) => {
                                                        println!("Disconnected from {:?}", addr);
                                                        let _ =
                                                            status.connections.remove(i as usize);
                                                    }
                                                    Err(e) => println!(
                                                        "Error sending message to {:?}: {:?}",
                                                        addr, e
                                                    ),
                                                }
                                            }
                                            i -= 1;
                                        }
                                    } else {
                                        let response = if let Some(current_page) =
                                            status.current_pages.get(comic)
                                        {
                                            json!({"comic": comic, "page": current_page})
                                        } else {
                                            json!({ "comic": comic })
                                        };
                                        let conn = &mut status
                                            .connections
                                            .iter_mut()
                                            .find(|el| el.0 == remote_addr)
                                            .unwrap()
                                            .1;
                                        conn.send(Message::Text(response.to_string()))
                                            .await
                                            .unwrap();
                                    }
                                }
                                Some(Ok(_)) => todo!(),
                                Some(Err(e)) => println!(
                                    "error creating stream on \
                                                connection from address {}. \
                                                Error is {}",
                                    remote_addr, e
                                ),
                            }
                        }
                    }
                    Err(e) => println!(
                        "error when trying to upgrade connection \
                                from address {} to websocket connection. \
                                Error is: {}",
                        remote_addr, e
                    ),
                }
            });
            //return the response to the handshake request
            response
        }
        Err(error) => {
            //probably the handshake request is not up to spec for websocket
            println!(
                "Failed to create websocket response \
                        to request from address {}. \
                        Error is: {}",
                remote_addr, error
            );
            let mut res =
                Response::new(Body::from(format!("Failed to create websocket: {}", error)));
            *res.status_mut() = StatusCode::BAD_REQUEST;
            return Ok(res);
        }
    };

    Ok(response)
}

async fn handle_static_request(url: &str) -> Result<Response<Body>, Response<Body>> {
    //handle any other url without an Upgrade header field
    lazy_static! {
        static ref ROUTER: Router<&'static str> = {
            let mut router = Router::new();
            router.add("/index.html", "index.html");
            router.add("/:comic/reader.html", "reader.html");
            router.add("/:comic/reader.js", "reader.js");
            router.add("/comic_list", "comic_list");
            router.add("/:comic/img_list", "img_list");
            router.add("/img/:comic/:img", "img");
            router
        };
    };

    let route = ROUTER.recognize(url).map_err(|_| err(404))?;

    match **route.handler() {
        "index.html" => {
            let data = fs::read_to_string("index.html").map_err(|_| err(500))?;
            Ok(Response::new(Body::from(data)))
        }
        "reader.html" => {
            let data = fs::read_to_string("reader.html").map_err(|_| err(500))?;
            Ok(Response::new(Body::from(data)))
        }
        "reader.js" => {
            let data = fs::read_to_string("reader.js").map_err(|_| err(500))?;
            Ok(Response::builder()
                .header("Content-Type", "text/javascript")
                .body(Body::from(data))
                .unwrap())
        }
        "comic_list" => {
            let mut folders = fs::read_dir(&CONFIG.img_folder)
                .map_err(|_| err(500))?
                .filter_map(|el| {
                    let info = el.ok()?;
                    if info.path().is_dir() {
                        Some(info.file_name().to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            folders.sort();
            Ok(Response::builder()
                .header("Content-Type", "text/json")
                .body(Body::from(json!(folders).to_string()))
                .unwrap())
        }
        "img_list" => {
            let comic = route.params().find("comic").ok_or(err(500))?;
            let mut path = CONFIG.img_folder.clone();
            path.push(comic);
            let mut files = fs::read_dir(path)
                .map_err(|_| err(500))?
                .filter_map(|el| Some(el.ok()?.file_name().to_string_lossy().into_owned()))
                .collect::<Vec<_>>();
            files.sort_by(|a, b| smart_cmp(a, b));
            let mut response = json!({
                "pages": files,
            });
            if let Some(mirror_url) = CONFIG.mirror.as_ref() {
                response["mirror"] = serde_json::Value::from(&**mirror_url);
            };
            Ok(Response::builder()
                .header("Content-Type", "text/json")
                .body(Body::from(response.to_string()))
                .unwrap())
        }
        "img" => {
            let comic = route.params().find("comic").ok_or(err(500))?;
            let img = decode(route.params().find("img").ok_or(err(500))?).map_err(|_| err(500))?;
            let mut path = CONFIG.img_folder.clone();
            path.push(comic);
            path.push(&*img);
            let data = fs::read(path).map_err(|_| err(500))?;
            Ok(Response::builder()
                .header("Cache-Control", "public, max-age=604800")
                .body(Body::from(data))
                .unwrap())
        }
        _ => Err(err(404)),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let response = Client::new()
        .get(Uri::from_static("http://ipinfo.io/ip"))
        .await
        .expect("Request for external IP failed.");
    if response.status() == StatusCode::OK {
        let mut buf = String::new();
        body::aggregate(response)
            .await
            .expect("Request for external IP couldn't be parsed.")
            .reader()
            .read_to_string(&mut buf)
            .unwrap();
        if let Ok(uri) = Uri::from_str(&buf) {
            println!("External link: http://{}:{}/index.html", uri, CONFIG.port);
        } else {
            println!("Unable to determine external IP.");
        }
    } else {
        println!("Unable to determine external IP.");
    }

    println!("{:?}", *CONFIG);

    //hyper server boilerplate code from https://hyper.rs/guides/server/hello-world/
    let addr = SocketAddr::from(([0, 0, 0, 0], CONFIG.port));

    println!("Listening on {} for http or websocket connections.", addr);

    // A `Service` is needed for every connection, so this
    // creates one from our `handle_request` function.
    let make_svc = make_service_fn(|conn: &AddrStream| {
        let remote_addr = conn.remote_addr();
        async move {
            // service_fn converts our function into a `Service`
            Ok::<_, Infallible>(service_fn(move |request: Request<Body>| {
                handle_request(request, remote_addr)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    // Run this server for... forever!
    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_smart_cmp() {
        assert_eq!(smart_cmp("ch10_p9.jpg", "ch2_p9.jpg"), Ordering::Greater);
        assert_eq!(smart_cmp("ch1_p8.jpg", "ch1_p9.jpg"), Ordering::Less);
        assert_eq!(smart_cmp("ch10_p9.jpg", "ch10_p9.jpg"), Ordering::Equal);
        assert_eq!(
            smart_cmp("c166 (v21) - p000.jpg", "c166 (v21) - p000x1.png"),
            Ordering::Less
        );
        assert_eq!(
            smart_cmp("c166 (v21) - p000x2.jpg", "c166 (v21) - p001.jpg"),
            Ordering::Less
        );
    }
}
