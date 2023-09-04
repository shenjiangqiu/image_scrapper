use std::{
    cell::RefCell,
    fs::{self, File},
    path::Path,
    rc::Rc,
    sync::Arc,
};

use chrono::{LocalResult, TimeZone, Utc};
use cli::{Cli, DownloadArgs, FixArgs, ListArgs, TranslateArgs};
use data::Data;
use reqwest::Client;
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use scraper::{node::Element, Html, Selector};
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::data::ImageInfo;
pub mod cli;
pub mod data;

fn parse_cookie_store(cookie_path: Option<&Path>) -> eyre::Result<CookieStore> {
    let path = cookie_path.ok_or(eyre::eyre!("no filepath provided"))?;
    let file = std::io::BufReader::new(File::open(path)?);
    let cookie_store = CookieStore::load_json(file)
        .map_err(|e| eyre::eyre!("fail to load the json file: err {:?}", e))?;
    Ok(cookie_store)
}

fn parse_data(data_path: Option<&Path>) -> eyre::Result<Data> {
    let path = data_path.ok_or(eyre::eyre!("no filepath provided"))?;
    let file = std::io::BufReader::new(File::open(path)?);
    let data = bincode::deserialize_from(file)?;
    Ok(data)
}
pub async fn run(args: Cli) -> eyre::Result<()> {
    match args.subcmd {
        cli::SubCommands::List(args) => list(args)?,
        cli::SubCommands::Download(args) => download(args).await?,
        cli::SubCommands::Fix(args) => fix(args).await?,
        cli::SubCommands::Translate(args) => translate(args).await?,
    }
    Ok(())
}

#[derive(Serialize, Deserialize, Debug)]
#[allow(non_snake_case)]
struct ChromeJsonFile {
    pub domain: String,
    pub expirationDate: f64,
    pub hostOnly: bool,
    pub httpOnly: bool,
    pub name: String,
    pub path: String,
    pub sameSite: Option<String>,
    pub secure: bool,
    pub session: bool,
    pub storeId: Option<String>,
    pub value: String,
}

#[derive(Debug, Deserialize,Serialize)]
struct Cookie {
    raw_cookie: String,
    path: Vec<serde_json::Value>,
    domain: Domain,
    expires: Expiration,
}

#[derive(Debug, Deserialize,Serialize)]
#[allow(non_snake_case)]
struct Domain {
    Suffix: String,
}

#[derive(Debug, Deserialize,Serialize)]
#[allow(non_snake_case)]
struct Expiration {
    AtUtc: String,
}

pub async fn translate(args: TranslateArgs) -> eyre::Result<()> {
    let json = match (args.input, args.file) {
        (None, None) => {
            eprintln!("No input provided, should either provide --input or --file");
            return Err(eyre::eyre!("No input provided"));
        }
        (Some(input), _) => input,
        (None, Some(file_path)) => {
            let file = fs::read_to_string(file_path)?;
            file
        }
    };
    let json: Vec<ChromeJsonFile> = serde_json::from_str(&json)?;
    // translate the json to my json
    let cookies = json.into_iter().map(|oc| Cookie {
        raw_cookie: format!("{}={}", oc.name, oc.value),
        path: vec![
            serde_json::Value::String(oc.path),
            serde_json::Value::Bool(true),
        ],
        domain: Domain { Suffix: oc.domain },
        expires: Expiration {
            AtUtc: {
                let expiration_timestamp = oc.expirationDate;
                let expiration_date = Utc.timestamp_opt(
                    expiration_timestamp as i64,
                    (expiration_timestamp.fract() * 1_000_000_000.0) as u32,
                );
                let utc = match expiration_date {
                    LocalResult::Single(i) => {
                        let expiration_str = i.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
                        // eprintln!("Expiration Date: {}", expiration_str);
                        expiration_str
                    }
                    _ => {
                        eprintln!("fail to parse the date");
                        panic!("fail to parse the date")
                    }
                };
                utc
            },
        },
    });
    for c in cookies{
        let cookie_str = serde_json::to_string(&c)?;
        println!("{}",cookie_str);
    }
    Ok(())
}
pub fn list(args: ListArgs) -> eyre::Result<()> {
    let ListArgs { data_path } = args;
    let data = parse_data(Some(data_path.as_ref()))?;
    for (k, v) in data.topisc.iter() {
        println!("key: {}", k);
        for info in v {
            if let Some(name) = &info.name {
                println!("  --name: {}", name);
            }
            println!("  --url: {}", info.url);
        }
    }
    Ok(())
}

pub async fn fix(args: FixArgs) -> eyre::Result<()> {
    let FixArgs {
        cookie_file,
        data_path,
    } = args;
    let cookie = parse_cookie_store(cookie_file.as_ref().map(AsRef::as_ref))
        .unwrap_or(CookieStore::default());
    let cookie = Arc::new(CookieStoreRwLock::new(cookie));
    let data = parse_data(Some(data_path.as_ref()))?;
    for (keys, values) in data.topisc {
        // first check if ./data/keys exists
        let path = Path::new("./data").join(&keys);
        if !path.exists() {
            println!("{} not exists, start to download", keys);
            std::fs::create_dir_all(&path)?;
            let mut handles = vec![];
            let client = reqwest::Client::builder()
                .cookie_store(true)
                .cookie_provider(cookie.clone())
                .build()?;
            let client = Rc::new(client);
            let local_set = tokio::task::LocalSet::new();
            local_set
                .run_until(async {
                    for info in values {
                        // TODO: handle 'static problem, maybe find a scoped async spawn
                        let path = path.to_owned();
                        let client = Rc::clone(&client);

                        handles.push(tokio::task::spawn_local(async move {
                            let file_name = info
                                .name
                                .as_deref()
                                .unwrap_or(info.url.split('/').last().unwrap());
                            println!("downloading the img {:?}", file_name);
                            let req = client.get(&info.url).build()?;
                            let result = client.execute(req).await?;
                            // save the img
                            let bytes = result.bytes().await?;

                            let mut file = tokio::io::BufWriter::new(
                                tokio::fs::File::create(path.join(file_name)).await?,
                            );
                            println!("saving the img {:?}", file_name);
                            file.write_all(&bytes).await?;

                            Ok::<(), eyre::Error>(())
                        }));
                    }
                    for handle in handles {
                        handle.await??;
                    }
                    Ok::<(), eyre::Error>(())
                })
                .await?;
        } else {
            println!("{} exists, skip", keys);
        }
    }
    Ok(())
}

pub async fn download(args: DownloadArgs) -> eyre::Result<()> {
    if args.url.is_empty() {
        println!("No url provided");
        return Err(eyre::eyre!("No url provided"))?;
    }
    let mut changed = false;
    let data = parse_data(args.data_path.as_ref().map(AsRef::as_ref)).unwrap_or(Data::default());

    let cookie_store = parse_cookie_store(args.cookie_file.as_ref().map(AsRef::as_ref))
        .unwrap_or(CookieStore::default());

    let cookie_store = Arc::new(CookieStoreRwLock::new(cookie_store));
    let client = reqwest::Client::builder()
        .cookie_store(true)
        .cookie_provider(Arc::clone(&cookie_store))
        .build()?;
    let mut handles = vec![];
    let client = Rc::new(client);
    let data = Rc::new(RefCell::new(data));
    let local_set = tokio::task::LocalSet::new();
    println!("running the urls");
    local_set
        .run_until(async {
            for url in args.url {
                handles.push(tokio::task::spawn_local(run_single_url(
                    url,
                    client.clone(),
                    data.clone(),
                )));
            }
            for handle in handles {
                match handle.await? {
                    Ok(c) => changed |= c,
                    Err(error) => {
                        println!("error: {:?}", error);
                    }
                }
            }
            Ok::<(), eyre::Error>(())
        })
        .await?;

    // save the cookies
    if let Some(file) = args.cookie_file {
        println!("saving the cookies to {:?}", file);
        let mut file = File::create(file)?;
        cookie_store
            .write()
            .map_err(|_| eyre::eyre!("fail to write"))?
            .save_json(&mut file)
            .map_err(|_| eyre::eyre!("fail to save"))?;
    }
    // save the data
    if changed {
        if let Some(file) = args.data_path {
            println!("saving the data to {:?}", file);
            let file = File::create(file)?;
            let file = std::io::BufWriter::new(file);
            bincode::serialize_into(file, &*data.borrow())?;
        }
    } else {
        println!("no change, no need to save the data");
    }
    Ok(())
}
fn get_img_src(ele: &Element) -> &str {
    let img_src = ele.attr("src").unwrap();
    let img_click = ele.attr("onclick");
    let re = regex::Regex::new(r#"^Previewurl\('(.*)'\)$"#).unwrap();
    if let Some(click) = img_click {
        let cap = re.captures(click);
        if let Some(cap) = cap {
            return cap.get(1).unwrap().as_str();
        } else {
            return img_src;
        }
    } else {
        return img_src;
    }
}
async fn run_single_url(
    url: String,
    client: Rc<Client>,
    data: Rc<RefCell<Data>>,
) -> eyre::Result<bool> {
    let req = client.get(&url).build()?;
    let result = client.execute(req).await?;
    let text = result.text().await?;
    let html = Html::parse_document(&text);
    let selector = Selector::parse("img").unwrap();
    let url = Url::parse(&url)?;
    // first test the database
    let mut total_digest = md5::compute(b"start");
    for element in html.select(&selector) {
        let img_src = get_img_src(element.value());
        let img_url = url.join(img_src)?;
        let src_md5 = md5::compute(img_url.as_str());
        for i in total_digest.0.iter_mut().zip(src_md5.0.iter()) {
            *i.0 ^= *i.1;
        }
    }
    let total_digest_string = format!("{:x}", total_digest);
    if data.borrow().topisc.contains_key(&total_digest_string) {
        println!("already downloaded");
        return Ok(false);
    }
    println!("not downloaded yet, start to download");
    let img_path = Path::new("./data").join(&total_digest_string);
    // makedirs
    std::fs::create_dir_all(&img_path)?;
    let mut srcs = vec![];
    let mut handles = vec![];
    for element in html.select(&selector) {
        //get the img src
        let img_src = get_img_src(element.value());
        let img_alt = element.value().attr("alt");
        let img_name = img_src.split('/').last().unwrap();
        let img_name = img_alt.filter(|v| is_image_name(v)).unwrap_or(img_name);
        let img_path = img_path.join(&img_name);

        // get the img url
        let img_url = url.join(img_src)?;
        srcs.push(ImageInfo {
            name: Some(img_name.to_string()),
            url: img_url.to_string(),
        });
        // get and save the img
        let req = client.get(img_url).build()?;
        let client_c = Rc::clone(&client);
        handles.push(tokio::task::spawn_local(async move {
            println!("downloading the img {:?}", img_path);
            let result = client_c.execute(req).await?;
            let bytes = result.bytes().await?;
            let mut file = tokio::io::BufWriter::new(tokio::fs::File::create(&img_path).await?);
            println!("saving the img {:?}", img_path);
            file.write_all(&bytes).await?;
            Ok::<(), eyre::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    println!("saving the database");
    data.borrow_mut().topisc.insert(total_digest_string, srcs);

    Ok(true)
}

fn is_image_name(v: &str) -> bool {
    let re = regex::Regex::new(r#"^.*\.(jpg|png|jpeg)$"#).unwrap();
    re.is_match(v)
}

#[cfg(test)]
mod tests {
    use chrono::LocalResult;
    use chrono::TimeZone;
    use chrono::Utc;
    #[test]
    fn test_chrono() {
        let expiration_timestamp: f64 = 1723197836.013598;

        // Convert the timestamp to a DateTime<Utc> object
        let expiration_date = Utc.timestamp_opt(
            expiration_timestamp as i64,
            (expiration_timestamp.fract() * 1_000_000_000.0) as u32,
        );
        if let LocalResult::Single(i) = expiration_date {
            let expiration_str = i.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            println!("Expiration Date: {}", expiration_str);
        }
    }
}
