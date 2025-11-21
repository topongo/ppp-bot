use std::{fs::read_to_string, io::{Read, Write}, path::Path, process::exit};

use lazy_static::lazy_static;
use log::{debug, error};
use mongodb::options::ClientOptions;
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Config {
    pub db: DbConfig,
    pub tg: TgConfig,
    pub import: ImportConfig,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DbConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(default = "db_password_from_env_or_file")]
    pub password: String,
}

impl DbConfig {
    pub fn client(&self) -> mongodb::Client {
        mongodb::Client::with_options(ClientOptions::builder()
            .hosts(vec![format!("{}:{}", self.host, self.port).parse().expect("Failed to parse host")])
            .credential(mongodb::options::Credential::builder()
                .username(self.user.clone())
                .password(self.password.clone())
                .build())
            .build()
        ).expect("Failed to create client")
    }
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            host: "localhost".to_owned(),
            port: 27017,
            user: "ppp".to_owned(),
            password: "hackmeeee".to_owned(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct TgConfig {
    #[serde(default = "token_from_env")]
    pub token: String,
    pub admin: String,
}

fn token_from_env() -> String {
    std::env::var("PPP_TOKEN")
        .or_else(|_| std::env::var("PPP_TOKEN_FILE").map(|f| read_to_string(f)
            .expect("could not read token from file")
            .trim()
            .to_string()
        ))
        .expect("could not get token from environment")
}

fn db_password_from_env_or_file() -> String {
    std::env::var("PPP_DB_PASSWORD")
        .or_else(|_| std::env::var("PPP_DB_PASSWORD_FILE").map(|f| read_to_string(f)
            .expect("could not read db password file")
            .trim()
            .to_string()
        ))
        .expect("could not get db password from environment")
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ImportConfig {
    pub show_id: u32,
    pub download_dir: String,
    pub wav_dir: String,
    pub transcript_dir: String,
    pub transcriber_url: String,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            show_id: 0,
            download_dir: "audio/mp3".to_owned(),
            wav_dir: "audio/wav".to_owned(),
            transcript_dir: "transcripts".to_owned(),
            transcriber_url: "http://localhost:8080/inference".to_owned(),
        }
    }
}

impl ImportConfig {
    pub fn check_dirs(&self) -> bool {
        [&self.download_dir, &self.wav_dir, &self.transcript_dir].iter()
            .all(|d| {
                debug!("checking {}", d);
                Path::new(d).exists()
            })
    }
}

impl Config {
    fn from_file() -> Self {
        match std::fs::File::open("config.toml") {
            Ok(mut f) => {
                let mut buf = String::new();
                f.read_to_string(&mut buf).expect("Failed to read config file");
                toml::from_str(&buf).expect("Failed to parse config file")
            }
            Err(_) => {
                let mut f = std::fs::File::create("config.toml").expect("Failed to create config file");
                let d = Self::default();
                f.write_all(toml::to_string(&d).expect("Failed to serialize default config").as_bytes()).expect("Failed to write default config");
                eprintln!("Failed to open config file, writing default config");
                exit(1)
            },
        }
    }
}

lazy_static!{
    pub static ref CONFIG: Config = Config::from_file();
}
