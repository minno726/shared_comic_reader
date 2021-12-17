use hyper::{body, Client, StatusCode, Uri};
use lazy_static::lazy_static;
use std::{fs, io::Read, str::FromStr};
use warp::{filters::BoxedFilter, reply::Json, Buf, Filter};

mod config;
mod sharing_service;
mod smart_cmp;
use config::Config;

lazy_static! {
    static ref CONFIG: Config = Config::init_from_environment();
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    pretty_env_logger::init();

    let router = ws().or(warp::get().and(apis().or(static_files())));

    print_external_address().await;

    warp::serve(router).run(([0, 0, 0, 0], CONFIG.port)).await;

    println!("Done!");
}

async fn print_external_address() {
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
            println!("External link: http://{}:{}/", uri, CONFIG.port);
        } else {
            println!("Unable to determine external IP.");
        }
    } else {
        println!("Unable to determine external IP.");
    }
}

fn ws() -> BoxedFilter<(impl warp::Reply,)> {
    warp::path("msg")
        .and(warp::ws())
        .map(|ws: warp::ws::Ws| ws.on_upgrade(sharing_service::on_upgrade))
        .boxed()
}

fn static_files() -> BoxedFilter<(impl warp::Reply,)> {
    let index = warp::path::end().and(warp::fs::file("index.html"));
    let reader_script = warp::path!("reader.js").and(warp::fs::file("reader.js"));
    let reader = warp::path!("read" / String)
        .map(|_| ())
        .untuple_one()
        .and(warp::fs::file("reader.html"));
    reader_script.or(reader).or(index).boxed()
}

fn apis() -> BoxedFilter<(impl warp::Reply,)> {
    let comic_list = warp::path!("comic_list").map(comic_list);
    let img_list = warp::path!("img_list" / String).map(img_list);
    let img = warp::path!("img" / ..).and(warp::fs::dir(CONFIG.img_folder.clone()));

    comic_list.or(img_list).or(img).boxed()
}

fn comic_list() -> Json {
    let mut folders = fs::read_dir(&CONFIG.img_folder)
        .unwrap()
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
    warp::reply::json(&folders)
}

fn img_list(comic: String) -> Json {
    let mut path = CONFIG.img_folder.clone();
    path.push(comic);
    let mut files = fs::read_dir(path)
        .unwrap()
        .filter_map(|el| Some(el.ok()?.file_name().to_string_lossy().into_owned()))
        .collect::<Vec<_>>();
    files.sort_by(|a, b| smart_cmp::smart_cmp(a, b));
    let mut response = serde_json::json!({
        "pages": files,
    });
    if let Some(mirror_url) = CONFIG.mirror.as_ref() {
        response["mirror"] = serde_json::Value::from(&**mirror_url);
    };
    warp::reply::json(&response)
}
