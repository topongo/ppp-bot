use std::fmt::{self, Display, Formatter};

use super::{interaction::InteractionError, search::SearchError};

#[derive(Debug)]
pub enum BotError {
    Mongo(mongodb::error::Error),
    Serde(serde_json::Error),
    Teloxide(teloxide::RequestError),
    NotImplemented,
    SearchError(SearchError),
    MalformedQuery,
    Interaction(InteractionError),
}

impl BotError {
    pub fn respond_client(&self) -> String {
        if matches!(self, BotError::SearchError(_)) {
            match self {
                BotError::SearchError(e) => return e.respond_client().to_string(),
                _ => unreachable!(),
            }
        }
        let r = format!(
            "c'è stato un problema nel generare la risposta: {}",
            match self {
                BotError::Mongo(_) => "errore database",
                BotError::Serde(_) => "errore di serializzazione",
                BotError::Teloxide(_) => "errore telegram",
                BotError::NotImplemented => "questa funzionalità non è implementata",
                BotError::SearchError(e) => e.respond_client(),
                BotError::MalformedQuery => "query malformata",
                BotError::Interaction(_) => "errore interazione",
            },
        );
        #[cfg(debug_assertions)]
        let r = format!("{}: {:?}", r, self);
        r
    }
}

impl Display for BotError {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "BotError")
    }
}

impl std::error::Error for BotError {}

impl From<mongodb::error::Error> for BotError {
    fn from(e: mongodb::error::Error) -> Self {
        BotError::Mongo(e)
    }
}

impl From<serde_json::Error> for BotError {
    fn from(e: serde_json::Error) -> Self {
        BotError::Serde(e)
    }
}

impl From<teloxide::RequestError> for BotError {
    fn from(e: teloxide::RequestError) -> Self {
        BotError::Teloxide(e)
    }
}

impl From<SearchError> for BotError {
    fn from(e: SearchError) -> Self {
        BotError::SearchError(e)
    }
}

impl From<InteractionError> for BotError {
    fn from(e: InteractionError) -> Self {
        BotError::Interaction(e)
    }
}

