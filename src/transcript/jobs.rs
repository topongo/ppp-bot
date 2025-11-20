use std::io::Write;
use std::fmt::Display;
use std::sync::{Arc, Mutex};
use log::debug;
#[allow(unused_imports)]
use log::{error, info, warn};
use futures_util::stream::StreamExt;

use crate::config::CONFIG;
use crate::db::DB;
use crate::spreaker::Episode;
use crate::transcript::data::TranscriptAlt;
use tokio::sync::Semaphore;


use tokio::task::JoinHandle;

use super::data::{EpisodeTranscript, Transcript}; type JobContainer<T> = Mutex<Vec<JoinHandle<Result<T, JobManagerError>>>>;

pub struct JobManager {
    cli: Arc<reqwest::Client>,
    conv_sem: Arc<Semaphore>,
    tran_sem: Arc<Semaphore>,
    down_sem: Arc<Semaphore>,
    insd_sem: Arc<Semaphore>,
    conv_jobs: JobContainer<EpisodeTranscript>,
    tran_jobs: JobContainer<(u32, Transcript)>,
    down_jobs: JobContainer<u32>,
    insd_jobs: JobContainer<()>,
}

impl JobManager {
    pub fn new(cli: Arc<reqwest::Client>) -> Self {
        Self {
            cli,
            conv_sem: Arc::new(Semaphore::new(MAX_CONVERT_JOBS)),
            tran_sem: Arc::new(Semaphore::new(MAX_TRANSCRIBE_JOBS)),
            down_sem: Arc::new(Semaphore::new(MAX_DOWNLOAD_JOBS)),
            insd_sem: Arc::new(Semaphore::new(MAX_INSERT_DB_JOBS)),
            conv_jobs: Mutex::new(vec![]),
            tran_jobs: Mutex::new(vec![]),
            down_jobs: Mutex::new(vec![]),
            insd_jobs: Mutex::new(vec![]),
        }
    }

    pub fn run_convert(&self, id: u32, transcript: Transcript) {
        debug!("enqueuing convert job for episode {}", id);
        let conv = Self::_run_convert(id, transcript, self.conv_sem.clone());
        let handle = tokio::spawn(conv);
        self.conv_jobs.lock().unwrap().push(handle);
    }

    pub fn run_transcribe(&self, id: u32) {
        debug!("enqueuing transcribe job for episode {}", id);
        let tran = Self::_run_transcribe(id, self.cli.clone(), self.tran_sem.clone());
        let handle = tokio::spawn(tran);
        self.tran_jobs.lock().unwrap().push(handle);
    }

    pub fn run_download(&self, id: u32) {
        debug!("enqueuing download job for episode {}", id);
        let down = Self::_run_download(id, self.cli.clone(), self.down_sem.clone());
        let handle = tokio::spawn(down);
        self.down_jobs.lock().unwrap().push(handle);
    }

    async fn _run_convert(id: u32, transcript: Transcript, sem: Arc<Semaphore>) -> Result<EpisodeTranscript, JobManagerError> {
        let _permit = sem.acquire().await.unwrap();
        info!("converting episode {}", id);
        let transcript = (id, transcript).into();
        drop(_permit);
        Ok(transcript)
    }

    async fn _run_transcribe(id: u32, cli: Arc<reqwest::Client>, sem: Arc<Semaphore>) -> Result<(u32, Transcript), JobManagerError> {
        let _permit = sem.acquire().await.unwrap();
        let f = format!("{}/{}.wav", CONFIG.import.wav_dir, id);
        // warn!("using file: {}", f);
        info!("transcribing espisode {}", id);
        let t = loop {
            match cli
                .post(CONFIG.import.transcriber_url.as_str())
                .multipart(reqwest::multipart::Form::new()
                    .text("temperature", "0.0")
                    .text("temperature_inc", "0.0")
                    .text("response_format", "verbose_json")
                    .file("file", &f).await?
                )
            // info!("sending request for episode {}: {:?}", id, req);
            // let t = req
                .send()
                .await {
                Ok(t) => break t.json::<TranscriptAlt>().await?,
                Err(e) => {
                    error!("error sending out request, retrying in 5 seconds: {}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        };
        let t: Transcript = t.into();
        let cache_f = format!("{}/{}.json", CONFIG.import.transcript_dir, id);
        debug!("writing transcript cache: {}", cache_f);
        let cache = std::fs::File::create(cache_f)?;
        serde_json::to_writer(cache, &t)?;
        drop(_permit);
        Ok((id, t))
    }
    
    async fn _run_download(id: u32, cli: Arc<reqwest::Client>, sem: Arc<Semaphore>) -> Result<u32, JobManagerError> {
        let _permit = sem.acquire().await.unwrap();
        info!("downloading episode {}", id);
        let e = DB.get::<Episode>(id).await?.unwrap();
        let url = e.download_url;
        let res = cli.get(&url).send().await?;
        let mp3 = format!("{}/{}.mp3", CONFIG.import.download_dir, e.id);
        debug!("download output: {}", mp3);
        let wav = format!("{}/{}.wav", CONFIG.import.wav_dir, e.id);
        debug!("wav output: {}", wav);
        if res.status().is_success() {
            let mut file = std::fs::File::create(&mp3)?;
            let mut stream =  res.bytes_stream();
            while let Some(item) = stream.next().await {
                let chunk = item?;
                file.write_all(&chunk)?;
            }
        }
        match tokio::process::Command::new("ffmpeg")
            .args(["-i", &mp3, "-ar", "16000", "-ac", "1", "-c:a", "pcm_s16le", &wav])
            .status()
            .await? {
            s if s.success() => {
                std::fs::remove_file(&mp3)?;
            }
            s => {
                error!("couldn't convert episode {} from mp3 to wav: {}", id, s);
            }
        }
        drop(_permit);
        Ok(id)
    }

    async fn _run_insert_db(e: EpisodeTranscript, sem: Arc<Semaphore>) -> Result<(), JobManagerError> {
        let _permit = sem.acquire().await.unwrap();
        info!("inserting episode {} into database", e.episode_id);
        DB.insert_stateless(&[e]).await?;
        drop(_permit);
        Ok(())
    }

    pub async fn wait(self) -> Result<(), JobManagerError> {
        for j in self.down_jobs.into_inner().unwrap().into_iter() {
            let id = j.await??;
            let job = Self::_run_transcribe(id, self.cli.clone(), self.tran_sem.clone());
            self.tran_jobs.lock()?.push(tokio::spawn(job));
        }

        for j in self.tran_jobs.into_inner().unwrap().into_iter() {
            let (id, t) = j.await??;
            let job = Self::_run_convert(id, t, self.conv_sem.clone());
            self.conv_jobs.lock()?.push(tokio::spawn(job));
        }

        for j in self.conv_jobs.into_inner().unwrap().into_iter() {
            let e = j.await??;
            let job = Self::_run_insert_db(e, self.insd_sem.clone());
            self.insd_jobs.lock()?.push(tokio::spawn(job));
        }

        for j in self.insd_jobs.into_inner().unwrap().into_iter() {
            j.await??;
        }

        Ok(())
    }
}

static MAX_CONVERT_JOBS: usize = 4;
static MAX_TRANSCRIBE_JOBS: usize = 1;
static MAX_DOWNLOAD_JOBS: usize = 4;
static MAX_INSERT_DB_JOBS: usize = 4;

#[derive(Debug)]
pub enum JobManagerError {
    Reqwest(reqwest::Error),
    Io(std::io::Error),
    Tokio(tokio::task::JoinError),
    Mongo(mongodb::error::Error),
    Mutex,
    Serde(serde_json::Error),
}

impl Display for JobManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Reqwest(e) => write!(f, "Reqwest error: {}", e),
            Self::Io(e) => write!(f, "IO error: {}", e),
            Self::Tokio(e) => write!(f, "Tokio error: {}", e),
            Self::Mutex => write!(f, "Mutex error"),
            Self::Mongo(e) => write!(f, "MongoDB error: {}", e),
            Self::Serde(e) => write!(f, "Serde error: {}", e),
        }
    }

}

impl std::error::Error for JobManagerError {}

impl From<reqwest::Error> for JobManagerError {
    fn from(e: reqwest::Error) -> Self {
        Self::Reqwest(e)
    }
}

impl From<std::io::Error> for JobManagerError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<tokio::task::JoinError> for JobManagerError {
    fn from(e: tokio::task::JoinError) -> Self {
        Self::Tokio(e)
    }
}

impl<T> From<std::sync::PoisonError<T>> for JobManagerError {
    fn from(_: std::sync::PoisonError<T>) -> Self {
        Self::Mutex
    }
}

impl From<mongodb::error::Error> for JobManagerError {
    fn from(e: mongodb::error::Error) -> Self {
        Self::Mongo(e)
    }
}

impl From<serde_json::Error> for JobManagerError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serde(e)
    }
}
