use std::fmt::Display;
use std::marker::PhantomData;
use mongodb::bson::doc;
use serde::{Deserialize, Serialize};
use teloxide::payloads::SendMessageSetters;

use teloxide::types::{InlineKeyboardButton, InlineKeyboardButtonKind, KeyboardButton, ReplyMarkup};
use teloxide::{prelude::Requester, types::ChatId, Bot};

use crate::db::{PPPDatabase, DB};

use super::BotUser;

#[derive(Serialize, Deserialize)]
pub struct Interaction<S> {
    chat: ChatId,
    user: BotUser,
    ty: InteractionType,
    state: PhantomData<S>,
}

#[derive(Serialize, Deserialize)]
enum InteractionType {
    Start,
    Support,
    Search(SearchType),
}

impl InteractionType {
    fn repr(&self) -> &'static str {
        match self {
            InteractionType::Start => "Aiuto",
            InteractionType::Support => "Supporto",
            InteractionType::Search(t) => match t {
                SearchType::Unset => panic!(),
                SearchType::Simple => "Ricerca",
                SearchType::Advanced => "Ricerca avanzata",
                SearchType::Episode => "Ricerca episodio",
            },
        }
    }

    fn button(&self) -> InlineKeyboardButton {
        // unwrap safe: we are serializing a known value
        InlineKeyboardButton::new(self.repr(), InlineKeyboardButtonKind::CallbackData(serde_json::to_string(self).unwrap()))
    }
}

impl InteractionType {
    fn keyboard() -> Vec<Vec<InlineKeyboardButton>> {
        vec![
            vec![InteractionType::Start.button(), InteractionType::Support.button()],
            vec![InteractionType::Search(SearchType::Simple).button(), InteractionType::Search(SearchType::Advanced).button()],
            vec![InteractionType::Search(SearchType::Episode).button()]
        ]
    }
}

#[derive(Serialize, Deserialize)]
enum SearchType {
    Unset,
    Simple,
    Advanced,
    Episode,
}

pub struct Created;
pub struct NeedType;
pub struct Dynamic(String);

impl Interaction<()> {
    pub fn new(chat: ChatId, user: BotUser, ty: InteractionType) -> Interaction<Created> {
        Interaction::<Created> {
            chat,
            user,
            ty,
            state: PhantomData,
        }
    }

    pub fn start(chat: ChatId, user: BotUser) -> Interaction<Created> {
        Self::new(chat, user, InteractionType::Start)
    }
}

impl<S> Interaction<S> {
    pub fn infer_state<T>(self) -> Interaction<T> where T: std::marker::Sync + std::marker::Send {
        Interaction::<T> {
            chat: self.chat,
            user: self.user,
            ty: self.ty,
            state: PhantomData,
        }
    }

    pub async fn already_exists(&self, bot: &Bot) -> InteractionResult<()> {
        bot.send_message(self.chat, "C'è già un'interazione in corso, usa il comando /cancel per annullarla")
            .await?;
        Ok(())
    }

    pub async fn cancel(self, bot: &Bot) -> InteractionResult<()> where S: std::marker::Sync + std::marker::Send {
        bot.send_message(self.chat, "Interazione annullata").await?;
        DB.interaction_delete(self).await?;
        Ok(())
    }
}

impl Interaction<Created> {
    pub async fn notify(self, bot: &Bot) -> InteractionResult<Interaction<NeedType>> {
        bot.send_message(self.chat, "Ciao, seleziona un azione")
            .reply_markup(ReplyMarkup::inline_kb(InteractionType::keyboard()))
            .await?;
        Ok(Self::infer_state(self))
    }
}

impl PPPDatabase {
    pub async fn interaction_get(&self, chat: &ChatId, user: &BotUser) -> Result<Option<Interaction<Dynamic>>, mongodb::error::Error> {
        self.db
            .collection::<Interaction<Dynamic>>("interactions")
            .find_one(doc! {"chat": chat.0, "user": user.id})
            .await
    }
    
    pub async fn interaction_delete<S>(&self, int: Interaction<S>) -> Result<(), mongodb::error::Error> where S: std::marker::Sync + std::marker::Send {
        self.db
            .collection::<Interaction<S>>("interactions")
            .delete_one(doc! {"chat": int.chat.0, "user": int.user.id})
            .await?;
        Ok(())
    }

    pub async fn interaction_save<S>(&self, int: Interaction<S>) -> Result<(), mongodb::error::Error> where S: std::marker::Sync + std::marker::Send {
        self.db
            .collection::<Interaction<S>>("interactions")
            .replace_one(doc!{"chat": int.chat.0, "user": int.user.id}, int)
            .upsert(true)
            .await?;
        Ok(())
    }
}

#[derive(Debug)]
pub enum InteractionError {
    Mongo(mongodb::error::Error),
    Teloxide(teloxide::RequestError),
}

impl Display for InteractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InteractionError::Mongo(e) => write!(f, "MongoDB error: {}", e),
            InteractionError::Teloxide(e) => write!(f, "Teloxide error: {}", e),
        }
    }
}

impl std::error::Error for InteractionError {}

impl From<mongodb::error::Error> for InteractionError {
    fn from(e: mongodb::error::Error) -> Self {
        InteractionError::Mongo(e)
    }
}

impl From<teloxide::RequestError> for InteractionError {
    fn from(e: teloxide::RequestError) -> Self {
        InteractionError::Teloxide(e)
    }
}

pub type InteractionResult<T> = Result<T, InteractionError>;

