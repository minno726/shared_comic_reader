use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use hyper::service::{make_service_fn, service_fn};
use hyper::upgrade::Upgraded;
use hyper::{header, upgrade, StatusCode};
use hyper::{server::conn::AddrStream, Body, Request, Response, Server};
use lazy_static::lazy_static;
use route_recognizer::Router;
use serde_json::json;
use std::cmp::Ordering;
use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
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

lazy_static! {
    static ref CONNECTIONS: Mutex<Vec<(SocketAddr, SplitSink<WebSocketStream<Upgraded>, Message>)>> =
        Mutex::new(Vec::new());
    static ref IMG_FOLDER: Mutex<PathBuf> = Mutex::new(PathBuf::new());
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

                        CONNECTIONS.lock().await.push((remote_addr, ws_write));
                        println!("Connected to {:?}", remote_addr);

                        loop {
                            match ws_read.next().await {
                                Some(Ok(Message::Close(_)))
                                | Some(Err(Error::ConnectionClosed))
                                | Some(Err(Error::Io(_)))
                                | None => break,
                                Some(Ok(msg @ Message::Text(_))) => {
                                    let mut connections = CONNECTIONS.lock().await;
                                    let mut i = connections.len() as isize - 1;
                                    while i >= 0 {
                                        let (addr, conn) = &mut connections[i as usize];
                                        if *addr != remote_addr {
                                            match conn.send(msg.clone()).await {
                                                Ok(()) => {}
                                                Err(Error::ConnectionClosed | Error::Io(_)) => {
                                                    println!("Disconnected from {:?}", addr);
                                                    let _ = connections.remove(i as usize);
                                                }
                                                Err(e) => println!(
                                                    "Error sending message to {:?}: {:?}",
                                                    addr, e
                                                ),
                                            }
                                        }
                                        i -= 1;
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
            router.add("/:comic/img/:img", "img");
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
            let mut folders = fs::read_dir(&*IMG_FOLDER.lock().await)
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
            let mut path = IMG_FOLDER.lock().await.clone();
            path.push(comic);
            let mut files = fs::read_dir(path)
                .map_err(|_| err(500))?
                .filter_map(|el| Some(el.ok()?.file_name().to_string_lossy().into_owned()))
                .collect::<Vec<_>>();
            files.sort_by(|a, b| smart_cmp(a, b));
            Ok(Response::builder()
                .header("Content-Type", "text/json")
                .body(Body::from(json!(files).to_string()))
                .unwrap())
        }
        "img" => {
            let comic = route.params().find("comic").ok_or(err(500))?;
            let img = decode(route.params().find("img").ok_or(err(500))?).map_err(|_| err(500))?;
            let mut path = IMG_FOLDER.lock().await.clone();
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
    let mut args = pico_args::Arguments::from_env();
    IMG_FOLDER
        .lock()
        .await
        .push(&args.value_from_str("--folder").unwrap_or(".".to_string()));
    let port = args
        .value_from_str("--port")
        .ok()
        .and_then(|s: String| s.parse::<u16>().ok())
        .unwrap_or(30000);

    //hyper server boilerplate code from https://hyper.rs/guides/server/hello-world/
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

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
