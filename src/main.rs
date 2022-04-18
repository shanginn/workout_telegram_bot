pub mod context;
pub mod lib;

use crate::context::{ContextCommand, ContextData, Contexts};
use chrono::prelude::*;
use chrono::Duration;
use frankenstein::{Api, GetUpdatesParams, GetUpdatesParamsBuilder, TelegramApi, Update};
use std::env;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Receiver;
use tokio::time;

#[tokio::main]
async fn main() {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(&token);
    let contexts = Arc::new(Mutex::new(Contexts::new(api.clone())));

    let cloned_contexts = Arc::clone(&contexts);
    let updates_handler = tokio::spawn(async move {
        get_all_updates(api, cloned_contexts).await;
    });

    let cloned_contexts = Arc::clone(&contexts);
    let daily_message_handler = tokio::spawn(async move {
        send_daily_messages(cloned_contexts).await;
    });

    tokio::try_join!(updates_handler, daily_message_handler).unwrap();
}

async fn send_daily_messages(contexts: Arc<Mutex<Contexts>>) {
    loop {
        let txs = &contexts.lock().unwrap().txs.clone();

        for (chat_id, context_tx) in txs {
            if context_tx.is_closed() {
                contexts.lock().unwrap().txs.remove_entry(chat_id);
            } else {
                context_tx.send(ContextCommand::SendDailyMessage).await;
            }
        }

        time::sleep(get_day_duration()).await;
    }
}

async fn get_all_updates(api: Api, contexts: Arc<Mutex<Contexts>>) {
    let update_delay = Duration::seconds(1).to_std().unwrap();

    let mut update_params: GetUpdatesParams = GetUpdatesParamsBuilder::default()
        .allowed_updates(strings_vec!["message", "edited_message"])
        .build()
        .unwrap();

    loop {
        time::sleep(update_delay).await;

        let result = api.get_updates(&update_params);

        println!("result: {:?}", result);

        match result {
            Ok(response) => {
                for update in response.result {
                    update_params.offset = Some(update.update_id + 1);
                    let (update, chat_id) = get_chat_id_from_update(update);

                    let chat_id = match chat_id {
                        Some(chat_id) => chat_id,
                        None => continue,
                    };

                    let txs = contexts.lock().unwrap().txs.clone();

                    if !txs.contains_key(&chat_id) {
                        if let Some(message) = update.message.clone() {
                            if let Some(text) = message.text {
                                if text == "/start" {
                                    println!("Initializing context {}", &chat_id);
                                    init_context(Arc::clone(&contexts), chat_id, api.clone());
                                }
                            }
                        }
                    }

                    let txs = contexts.lock().unwrap().txs.clone();

                    if txs.contains_key(&chat_id) {
                        let message = match update.message {
                            Some(message) => message,
                            None => continue,
                        };

                        if message.text.is_none() {
                            continue;
                        }

                        let text = message.text.unwrap();
                        let count = text.parse::<usize>();

                        let count = match count {
                            Ok(count) => count,
                            Err(err) => {
                                println!("Error parsing count: {:?}", err);
                                continue;
                            }
                        };

                        let username = message.from.unwrap().username.unwrap();

                        let tx = txs[&chat_id].clone();
                        tokio::spawn(async move {
                            tx.send(ContextCommand::AddPushups { username, count })
                                .await;
                        });
                    }
                }
            }
            Err(error) => {
                println!("Failed to get updates: {:?}", error);
            }
        }
    }
}

fn init_context(contexts: Arc<Mutex<Contexts>>, chat_id: i64, api: Api) {
    let (tx, rx) = mpsc::channel(2048);
    let cloned_tx = tx.clone();
    contexts.lock().unwrap().txs.insert(chat_id, cloned_tx);

    let context_data = ContextData::new(api, chat_id);

    tokio::spawn(async move { handle_commands(context_data, rx).await });

    tokio::spawn(async move { tx.send(ContextCommand::SendDailyMessage).await });
}

pub async fn handle_commands(mut context_data: ContextData, mut rx: Receiver<ContextCommand>) {
    while let Some(command) = rx.recv().await {
        match command {
            ContextCommand::SendDailyMessage => {
                context_data.unpin_daily_message();

                if context_data.is_workout_over() {
                    context_data.send_message(context_data.generate_final_message());
                    context_data.unpin_daily_message();
                    rx.close();

                    return;
                }

                let cycle_ended = context_data.init_next_day();
                if cycle_ended {
                    context_data.send_message(context_data.generate_end_of_cycle_message());
                }

                let text = context_data.generate_daily_message();

                if let Some(message) = context_data.send_message(text) {
                    context_data.daily_message_id = Some(message.message_id);
                    context_data.pin_daily_message();
                }
            }
            ContextCommand::AddPushups { username, count } => {
                context_data.add_user_progress(username.clone(), count);

                match context_data.update_daily_message() {
                    Ok(response) => println!("Edit ok: {:?}", response),
                    Err(err) => println!("Failed to update daily message: {:?}", err),
                }

                if context_data.is_user_done(username.clone()) {
                    context_data.send_message("🥳".to_string());
                }

                if context_data.is_all_users_done() {
                    context_data.send_message("На сегодня всё 🎉".to_string());
                }
            }
        }
    }
}

fn get_chat_id_from_update(update: Update) -> (Update, Option<i64>) {
    if update.message.is_some() {
        let chat_id = update.message.clone().unwrap().chat.id;

        (update, Some(chat_id))
    } else if update.edited_message.is_some() {
        let chat_id = update.edited_message.clone().unwrap().chat.id;

        (update, Some(chat_id))
    } else {
        (update, None)
    }
}

fn get_day_duration() -> core::time::Duration {
    //return Duration::seconds(5).to_std().unwrap();
    let now = Utc::now();
    let tomorrow_midnight = (now + Duration::days(1)).date().and_hms(0, 0, 0);

    tomorrow_midnight
        .signed_duration_since(now)
        .to_std()
        .unwrap()
}
