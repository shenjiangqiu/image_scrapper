use clap::Parser;
use image_scrapper::cli::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cli = Cli::parse();
    image_scrapper::run(cli).await.unwrap();
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{BufReader, BufWriter},
        path::{Path, PathBuf},
        str::FromStr,
        sync::Arc,
    };

    use image_scrapper::cli::{Cli, DownloadArgs};
    use reqwest;
    use reqwest_cookie_store::CookieStoreMutex;

    async fn make_request(url: &str, cookie_path: &Path) {
        let cookie_store = {
            let file = File::open(cookie_path).map(BufReader::new);
            match file {
                Ok(file) => reqwest_cookie_store::CookieStore::load_json(file).unwrap(),
                Err(_err) => reqwest_cookie_store::CookieStore::default(),
            }
        };
        let cookie_store = Arc::new(CookieStoreMutex::new(cookie_store));
        let client = reqwest::Client::builder()
            .cookie_store(true)
            .cookie_provider(cookie_store.clone())
            .build()
            .unwrap();
        let req = client.get(url).build().unwrap();
        println!("request headers: {:?}", req.headers());
        let result = client.execute(req).await.unwrap();

        println!("response body: {:?}", result.text().await.unwrap());
        // print the cookie
        let cookie_store = cookie_store.lock().unwrap();
        let cookies = cookie_store.iter_any().collect::<Vec<_>>();
        println!("cookies: {:?}", cookies);
        // store the cookies
        let mut file = File::create(cookie_path).map(BufWriter::new).unwrap();
        cookie_store.save_json(&mut file).unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_reqwest() {
        make_request("https://google.com", Path::new("./cookies.json")).await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_main() {
        let cli = Cli {
            subcmd: image_scrapper::cli::SubCommands::Download(DownloadArgs {
                cookie_file: Some(PathBuf::from_str("./cookies_nicept.json").unwrap()),
                url: vec!["https://www.nicept.net/fun.php".to_string()],
                data_path: Some(PathBuf::from_str("./data_nicept.bin").unwrap()),
            }),
        };
        image_scrapper::run(cli).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    async fn test_cookies() {
        make_request(
            "https://www.nicept.net/fun.php",
            Path::new("./cookies_test_cookies.json"),
        )
        .await;
    }
}
