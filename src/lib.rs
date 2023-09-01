use std::{
    cell::RefCell,
    fs::File,
    path::Path,
    rc::Rc,
    sync::Arc,
};

use cli::Cli;
use data::Data;
use reqwest::Client;
use reqwest_cookie_store::{CookieStore, CookieStoreRwLock};
use scraper::{Html, Selector, node::Element};
use tokio::io::AsyncWriteExt;
use url::Url;
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
                changed |= handle.await??;
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
fn get_img_src(ele:& Element)->&str{
    
    let img_src = ele.attr("src").unwrap();
    let img_click = ele.attr("onclick");
    let re = regex::Regex::new(r#"^Previewurl\('(.*)'\)$"#).unwrap();
    if let Some(click) = img_click{
        let cap=re.captures(click);
        if let Some(cap) = cap{
            return cap.get(1).unwrap().as_str();
        }else{
            return img_src;
        }
    }else{
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
        srcs.push(img_url.to_string());
        // get and save the img
        let req = client.get(img_url).build()?;
        let client_c = Rc::clone(&client);
        handles.push(tokio::task::spawn_local(async move{
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
