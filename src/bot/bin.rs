use std::{fmt::Display, time::Instant};

use log::{debug, error, info, trace};
use regex::Regex;
use teloxide::{dispatching::{HandlerExt, UpdateFilterExt, UpdateHandler}, dptree, prelude::{Dispatcher, Requester}, repls::CommandReplExt, types::{CallbackQuery, ChatId, Message, ParseMode, User, UserId}, utils::{command::BotCommands, markdown}, Bot};
use teloxide::prelude::Update;
use teloxide::payloads::SendMessageSetters;
use power_pizza_bot::{bot::{strings::HELP_MESSAGE, Interaction}, config::CONFIG};
use power_pizza_bot::{bot::{BotError, BotUser}, db::DB};

#[tokio::main]
async fn main() {
    pretty_env_logger::init();
    info!("ensuring database indexes");
    DB.ensure_index().await.expect("Failed to ensure index");

    let bot = Bot::new(CONFIG.tg.token.clone());
    log::info!("bot created, startring...");

    let schema = Update::filter_callback_query()
        .branch(teloxide::filter_command::<Command, _>().branch(dptree::endpoint(reply)))
        .branch(dptree::endpoint(callback_handler));

    Dispatcher::builder(bot, schema).build().dispatch().await;
}

fn represent_user(u: &Option<User>) -> String {
    match u {
        Some(u) => u.username.as_ref().unwrap_or(&u.first_name).to_owned(),
        None => "unknown".to_string()
    }
}

#[derive(BotCommands, Clone)]
#[command(description = "Sono supportati i seguenti messaggi:")]
enum Command {
    #[command(rename = "start")]
    Start,
    #[command(rename = "help")]
    Help,
    #[command(rename = "s", aliases = ["search", "c", "cerca"])]
    Search(String),
    #[command(rename = "sa", aliases = ["searchAdvanced", "cercaAvanzato", "ca"])]
    SearchAdvanced(String),
    #[command(rename = "sae", aliases = ["searchAdvancedEpisode", "cercaAvanzatoEpisodio", "cae"])]
    SearchAdvancedEpisode(String),
    #[command(rename = "beta")]
    Beta,
    #[command(rename = "betalist")]
    BetaList,
    #[command(rename = "betawaitlist")]
    BetaWaitList,
    #[command(rename = "betaaccept")]
    BetaAccept(String),
    #[command(rename = "cancel")]
    Cancel,
}

impl Command {
    fn admin_access(&self) -> bool {
        matches!(self, Self::BetaList | Self::BetaWaitList | Self::BetaAccept(..))
    }

    fn unrestricted(&self) -> bool {
        matches!(self, Self::Beta | Self::BetaList | Self::BetaWaitList | Self::BetaAccept(..))
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Command::Start => write!(f, "start"),
            Command::Help => write!(f, "help"),
            Command::Search(q) => write!(f, "search {}", q),
            Command::SearchAdvanced(q) => write!(f, "searchAdvanced {}", q),
            Command::SearchAdvancedEpisode(q) => write!(f, "searchAdvancedEpisode {}", q),
            Command::Beta => write!(f, "beta"),
            Command::BetaList => write!(f, "betaList"),
            Command::BetaWaitList => write!(f, "betaWaitList"),
            Command::BetaAccept(q) => write!(f, "betaAccept {}", q),
            Command::Cancel => write!(f, "cancel"),
        }
    }
}

async fn reply(bot: Bot, msg: Message, cmd: Command) -> Result<(), teloxide::RequestError> {
    info!("replying to command `{}` (id {}) from {}", cmd, msg.id, represent_user(&msg.from));
    match reply_inner(&bot, &msg, cmd.clone()).await {
        Ok(_) => info!("successfully replied to {} from {}", msg.id, represent_user(&msg.from)),
        Err(e) => {
            error!("failed to reply to message {} from {}: {:?}", msg.id, represent_user(&msg.from), e);
            bot.send_message(msg.chat.id, e.respond_client()).await?;
        }
    }
    Ok(())
}

async fn paginate_response(bot: &Bot, chat_id: ChatId, response: String) -> Result<(), BotError> {
    let mut message = String::with_capacity(4096);
    for chunk in response.split("\n\n") {
        if message.len() + chunk.len() + 2 > 4096 {
            bot.send_message(chat_id, &message).parse_mode(ParseMode::MarkdownV2).await?;
            message.clear();
        } else {
            message.push_str("\n\n");
        }
        message.push_str(chunk);
    }
    if !message.is_empty() {
        bot.send_message(chat_id, message).parse_mode(ParseMode::MarkdownV2).await?;
    }
    Ok(())
}

fn split_quoted_args(s: &str) -> Option<Vec<String>> {
    let mut args = vec![];
    let r = Regex::new(r#"("([^"]+)"|(\S+)")|(\S+)"#).unwrap();
    for cap in r.captures_iter(s) {
        let cap = match cap.get(2).or(cap.get(4)) {
            Some(c) => c,
            None => return None,
        };
        args.push(cap.as_str().to_string());
    }
    Some(args)
}

static MAX_RESULTS: usize = 50;

fn is_admin(u: &Option<User>) -> bool {
    if let Some(u) = u {
        u.username.as_ref().is_some_and(|u| *u == CONFIG.tg.admin)
    } else {
        false
    }
}

async fn reply_inner(bot: &Bot, msg: &Message, cmd: Command) -> Result<(), BotError> {
    let t = Instant::now();
    if !cmd.unrestricted() {
        if let Some(u) = msg.from.clone() {
            if !DB.whitelisted(u.id.0 as i64).await? {
                bot.send_message(msg.chat.id, "Ciao, mi dispiace ma il bot è attualmente in sviluppo. Grazie per l'interesse. Riceverai una notifica quando sarà pronto. Utilizza il comando /beta per richiedere ingresso in waitlist.").await?;
                return Ok(());
            }
        }
    }
    if cmd.admin_access() && !is_admin(&msg.from) {
        bot.send_message(msg.chat.id, "Non sei autorizzato a fare questa richiesta").await?;
        return Ok(());
    }
    match cmd {
        Command::Start => {
            if let Some(u) = msg.from.clone() {
                let u = BotUser::from(&u);
                match DB.interaction_get(&msg.chat.id, &u).await? {
                    Some(int) => {
                        info!("interaction already exists for user {}", represent_user(&msg.from));
                        int.already_exists(bot).await?;
                    }
                    None => {
                        info!("creating new interaction for user {}", represent_user(&msg.from));
                        let int = Interaction::start(msg.chat.id, u);
                        let int = int.notify(bot).await?;
                        DB.interaction_save(int).await?;
                    }
                }
            } else {
                bot.send_message(msg.chat.id, "C'è stato un errore nel processare la tua richiesta").await?;
            }
        }
        Command::Cancel => {
            if let Some(u) = msg.from.clone() {
                let u = BotUser::from(&u);
                match DB.interaction_get(&msg.chat.id, &u).await? {
                    Some(int) => {
                        info!("cancelling interaction for user {}", represent_user(&msg.from));
                        int.cancel(bot).await?;
                    }
                    None => {
                        info!("no interaction to cancel for user {}", represent_user(&msg.from));
                        bot.send_message(msg.chat.id, "Non c'è nessuna interazione da annullare").await?;
                    }
                }
            } else {
                bot.send_message(msg.chat.id, "C'è stato un errore nel processare la tua richiesta").await?;
            }
        }
        Command::Help => {
            bot.send_message(msg.chat.id, &*HELP_MESSAGE)
                .parse_mode(ParseMode::MarkdownV2)
                .await?;
        }
        Command::Search(query) => {
            if query.len() < 3 {
                bot.send_message(msg.chat.id, "La query deve essere di almeno 3 caratteri").await?;
            } else {
                let results = DB.search_meta(query).await?;
                if results.len() > MAX_RESULTS {
                    bot.send_message(msg.chat.id, format!("Troppi risultati trovati ({}), per favore affina la ricerca", results.len())).await?;
                    return Ok(());
                }
                let response = results
                    .iter()
                    .map(|r| format!(
                            "{}: {}", 
                            markdown::escape(&r.episode.id.to_string()),
                            markdown::link(&format!("https://www.spreaker.com/episode/{}", r.episode.id), &markdown::escape(&r.episode.title))
                    ))
                    .collect::<Vec<_>>()
                    .join("\n");
                paginate_response(bot, msg.chat.id, response).await?;
            }
        }
        Command::SearchAdvanced(query) => {
            info!("received search query: {}", query);
            bot.send_message(msg.chat.id, "Searching...").await?;
            debug!("querying db");
            let results = DB.search_transcript_all(query).await?;
            debug!("found {} results", results.len());
            if results.len() > MAX_RESULTS {
                bot.send_message(msg.chat.id, format!("Troppi risultati trovati ({}), per favore affina la ricerca", results.len())).await?;
                return Ok(());
            }
            let response = format!(
                "{}\n{}",
                markdown::escape("Found episodes:"),
                results
                    .iter()
                    .map(|r| format!(
                            "{}: {}", 
                            markdown::escape(&r.episode.id.to_string()),
                            markdown::link(&format!("https://www.spreaker.com/episode/{}", r.episode.id), &markdown::escape(&r.episode.title))
                    ))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            paginate_response(bot, msg.chat.id, response).await?;
        }
        Command::SearchAdvancedEpisode(query) => {
            bot.send_message(msg.chat.id, "searching episode transcripts...").await?;
            let args = split_quoted_args(&query).ok_or(BotError::MalformedQuery)?;
            let id = DB.magic_episode_search(args
                .first()
                .ok_or(BotError::MalformedQuery)?.to_string()).await?;
            let query = args
                .get(1)
                .ok_or(BotError::MalformedQuery)?
                .to_string();

            info!("parsed arguments: id: {}, query: {}", id, query.as_str());
            let results = DB.search_transcript_one(id, query.as_str().to_string()).await?;
            if results.len() > MAX_RESULTS {
                bot.send_message(msg.chat.id, format!("Troppi risultati trovati ({}), per favore affina la ricerca", results.len())).await?;
                return Ok(());
            }
            if results.matches.is_empty() {
                bot.send_message(msg.chat.id, "No matches found").await?;
            } else {
                let response = format!("{}{}\n{}",
                    markdown::escape("Risultati per "),
                    markdown::link(&format!("https://www.spreaker.com/episode/{}", results.episode.id), &markdown::escape(&results.episode.title)),
                    results.matches
                        .iter()
                        .map(|m| format!(
                            "{}\n{}",
                            markdown::escape(&format!("{:02}:{:02} - {:02}:{:02}",
                                m.time.from.as_secs() / 60,
                                m.time.from.as_secs() % 60,
                                m.time.to.as_secs() / 60,
                                m.time.to.as_secs() % 60
                            )),
                            markdown::blockquote(&markdown::escape(&format!("...{}...", m.hint)))
                        ))
                        .collect::<Vec<_>>()
                        .join("\n\n")
                );
                paginate_response(bot, msg.chat.id, response).await?;
            } 
        }
        Command::Beta => {
            info!("user {} requested beta access", represent_user(&msg.from));
            match &msg.from {
                Some(u) => {
                    match DB.get::<BotUser>(u.id.0 as i64).await? {
                        Some(mut user) => {
                            if user.beta {
                                info!("user {} already has beta access", user.identify());
                                bot.send_message(msg.chat.id, "Hai già accesso alla beta!").await?;
                            } else if user.waitlist {
                                info!("user {} already requested beta access", user.identify());
                                bot.send_message(msg.chat.id, "Hai già inviato una richiesta!").await?;
                            } else {
                                user.waitlist = true;
                                info!("inserting user {} into waitlist", user.identify());
                                DB.update_one_stateless(user.id, &user).await?;
                                bot.send_message(msg.chat.id, "Richiesta di entrare in beta inviata").await?;
                            }
                        }
                        None => {
                            let mut user = BotUser::from(u);
                            user.waitlist = true;
                            info!("inserting user {} into waitlist", user.identify());
                            DB.update_one_stateless(user.id, &user).await?;
                            bot.send_message(msg.chat.id, "Richiesta di entrare in beta inviata").await?;
                        }
                    }
                }
                None => {
                    bot.send_message(msg.chat.id, "C'è stato un errore nel processare la tua richiesta").await?;
                }
            }
        }
        Command::BetaWaitList | Command::BetaList => {
            let list = if matches!(cmd, Command::BetaWaitList) {
                DB.waitlist().await?
            } else {
                DB.beta_list().await?
            };

            bot.send_message(msg.chat.id, format!(
                "{}\n{}", 
                markdown::escape(&format!("{} ({}):", cmd, list.len())),
                list.iter().map(|u| format!(
                    "{} \\({}\\)",
                    u.user_or_name(),
                    markdown::link(&format!(
                        "tg://bot_command?command=betaaccept {}",
                        u.id
                    ), &u.id.to_string())
                )).collect::<Vec<String>>().join("\n")
            )).await?;
        }
        Command::BetaAccept(query) => { 
            let id = query.parse::<i64>().map_err(|_| BotError::MalformedQuery)?;
            let mut user = DB.get::<BotUser>(id).await?.ok_or(BotError::MalformedQuery)?;
            if user.beta {
                bot.send_message(msg.chat.id, "User already in beta").await?;
                return Ok(());
            } else {
                user.beta = true;
                let id = user.id;
                DB.update_one_stateless(id, &user).await?;
                info!("sending beta accepted to user {}", user.identify());
                bot.send_message(UserId(id as u64), "Richiesta di entrare in beta accettata!").await?;
                bot.send_message(msg.chat.id, format!("User {} accepted into beta", id)).await?;
            }
        }
    };
    trace!("replied in {:?}", t.elapsed());

    Ok(())
}


async fn callback_handler(bot: Bot, query: CallbackQuery) -> Result<(), teloxide::RequestError> {
    info!("received callback query: {:?}", query);
    // match reply_inner(&bot, &msg, cmd.clone()).await {
    //     Ok(_) => info!("successfully replied to {} from {}", msg.id, represent_user(&msg.from)),
    //     Err(e) => {
    //         error!("failed to reply to message {} from {}: {:?}", msg.id, represent_user(&msg.from), e);
    //         bot.send_message(msg.chat.id, e.respond_client()).await?;
    //     }
    // }
    Ok(())
}

async fn callback_handler_inner(bot: Bot, query: String, msg: Message) -> Result<(), BotError> {
    todo!()
}

