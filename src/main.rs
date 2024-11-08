use std::{collections::HashMap, env, fs::File, io::Write, process::Command};

use clap::Parser;
use regex::Regex;
use reqwest::{
    blocking::Client,
    header::{HeaderMap, HeaderValue, USER_AGENT},
};
use serde_json::Value;
use url::Url;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Args {
    #[arg(short, long, default_value = Some("sing-box"), value_name = "TYPE")]
    client: Option<String>,
    #[arg(short, long, value_name = "SUBSCRIPTION")]
    url: String,
}

fn check_url(sub_url: &str) -> Result<String, String> {
    let sub_url = sub_url.to_lowercase();

    match Url::parse(&sub_url) {
        Ok(parsed_url) => {
            if !["http", "https"].contains(&parsed_url.scheme()) {
                return Err(String::from("Only support http or https."));
            }

            if parsed_url.host_str().is_none() {
                return Err(String::from("Invalid url without host name."));
            }

            let url_regex = Regex::new(r"^https?://[-a-zA-Z0-9@:%._\+~#=]{2,256}\.[a-z]{2,6}\b([-a-zA-Z0-9@:%_\+.~#?&//=]*)$").unwrap();
            if !url_regex.is_match(&sub_url) {
                return Err(format!("Invalid url, please check again."));
            }

            Ok(sub_url)
        }
        Err(e) => Err(format!("URL parse failed: {e}")),
    }
}

fn fetch_subscription(sub_url: &str) -> Result<HashMap<String, Value>, Box<dyn std::error::Error>> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static("sing-box/1.6.0"));
    let client = Client::builder().default_headers(headers).build()?;
    let response = client.get(sub_url).send()?;

    if response.status().is_success() {
        let json_resp: Value = response.json()?;
        let data: std::collections::HashMap<String, Value> = serde_json::from_value(json_resp)?;
        Ok(data)
    } else {
        Err(format!("Error fetching subscription: HTTP {}", response.status()).into())
    }
}

#[derive(Debug, Default)]
struct ExternalController {
    address: String,
    port: String,
}

fn save_config(
    mut data: HashMap<String, Value>,
) -> Result<ExternalController, Box<dyn std::error::Error>> {
    let inbounds = data.get("inbounds");

    if inbounds.is_none() {
        return Err(format!("Can't find any inbounds in target configuration.").into());
    }

    let mut controller_info = ExternalController::default();
    let mut new_inbound = Vec::new();
    for inbound in inbounds.unwrap().as_array().unwrap() {
        let inbound_map: std::collections::HashMap<String, Value> =
            serde_json::from_value(inbound.clone()).unwrap();
        if inbound_map.get("type").is_some()
            && inbound_map
                .get("type")
                .unwrap()
                .as_str()
                .unwrap()
                .eq("mixed")
        {
            new_inbound.push(inbound.clone());
            controller_info.address = inbound_map
                .get("listen")
                .unwrap()
                .as_str()
                .unwrap()
                .to_string();
            controller_info.port = inbound_map.get("listen_port").unwrap().to_string();
        }
    }

    data.insert(String::from("inbounds"), Value::Array(new_inbound));

    let output_config = serde_json::to_string_pretty(&data)?;

    let mut file = File::create("config.json")?;
    file.write_all(output_config.as_bytes())?;
    println!("✅ Conver successfully, save to: config.json");
    Ok(controller_info)
}

fn make_external_config(
    controller: ExternalController,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut output = "External = external, ".to_string();

    let exec = Command::new("which").arg("sing-box").output()?;

    if exec.status.success() {
        output.push_str(&format!(
            "exec = \"{}\", ",
            String::from_utf8_lossy(&exec.stdout).trim()
        ));
    } else {
        println!("✖ sing-box not found, try install...");
        let install_sing_box = Command::new("brew")
            .arg("install")
            .arg("sing-box")
            .output()?;

        if install_sing_box.status.success() {
            println!("✅ Successfully installed sing-box");
            let exec = Command::new("which").arg("sing-box").output()?;
            output.push_str(&format!(
                "exec = \"{}\", ",
                String::from_utf8_lossy(&exec.stdout).trim()
            ));
        } else {
            return Err(
                "✖ Failed to install sing-box, please try: brew install sing-box."
                    .to_string()
                    .into(),
            );
        }
    }

    output.push_str(&format!("local-port = {}, ", controller.port));
    output.push_str("args = \"run\", ");
    output.push_str("args = \"-c\", ");
    output.push_str(&format!(
        "args = \"{}\", ",
        env::current_dir()?.join("config.json").display()
    ));
    output.push_str(&format!("address = {}", controller.address));
    Ok(output)
}

fn main() {
    let cli = Args::parse();

    if let Some(client_name) = cli.client.as_deref() {
        println!("✅ Target client type is: {client_name}")
    }

    let sub_url = check_url(&cli.url).unwrap_or_else(|e| {
        println!("{e}");
        std::process::exit(1);
    });
    // TODO: mark real url.
    println!("✅ Targe subscription url is: {sub_url}");

    let data = match fetch_subscription(&sub_url) {
        Ok(json_resp) => {
            println!("✅ Successfully fetched and parsed JSON.");
            json_resp.to_owned()
        }
        Err(e) => {
            println!("✖ Error: {e}");
            std::process::exit(1);
        }
    };

    let controller_info = match save_config(data) {
        Ok(controller) => {
            println!("✅ Successfully convert subscription.");
            controller
        }
        Err(e) => {
            println!("✖ Error: {e}");
            std::process::exit(1);
        }
    };

    let external_proxy = match make_external_config(controller_info) {
        Ok(external_info) => external_info,
        Err(e) => {
            println!("✖ Error: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "✅ Target surge external config:\n[Proxy]\n{}",
        external_proxy
    )
}
