use chrono::prelude::*;
use chrono::Duration;
use frankenstein::{EditMessageResponse, EditMessageTextParams, Error, MethodResponse, PinChatMessageParams, TelegramApi, UnpinAllChatMessagesParams, UnpinChatMessageParams};
use frankenstein::{Api, GetUpdatesParams};
use frankenstein::{GetUpdatesParamsBuilder, SendMessageParams};
use frankenstein::{Message, SendMessageParamsBuilder, EditMessageTextParamsBuilder, PinChatMessageParamsBuilder, UnpinChatMessageParamsBuilder, UnpinAllChatMessagesParamsBuilder};
use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use tokio::time;

const CHAT_ID: i64 = -1001559532421;

struct ContextData {
    daily_message_id: Option<i32>,
    current_day: usize,
    duration: usize,
    repeats: usize,
    progress: Vec<HashMap<String, usize>>,
    users: Vec<String>,
    api: Option<Api>
}

struct Context {
    data: Mutex<ContextData>
}

impl Default for Context {
    fn default() -> Self {
        Context{
            data: Mutex::new(ContextData {
                daily_message_id: None,
                current_day: 0,
                duration: 3,
                repeats: 100,
                progress: vec![HashMap::new()],
                users: vec![],
                api: None
            })
        }
    }
}

impl Context {
    pub fn is_user_done(&self, username: String) -> bool {
        let data = self.data.lock().unwrap();

        data.progress[data.current_day].get(&username).unwrap_or(&0) >= &data.repeats
    }

    pub fn is_all_users_done(&self) -> bool {
        let data = self.data.lock().unwrap();
        let daily_repeats = &data.repeats;

        for user_repeats in data.progress[data.current_day].values() {
            if user_repeats < daily_repeats {
                return false;
            }
        }

        true
    }

    pub fn add_user_progress(&self, username: String, count: usize) {
        let current_day = self.data.lock().unwrap().current_day.clone();
        let data = &mut self.data.lock().unwrap();

        if !data.users.contains(&username) {
            data.users.push(username.clone());
        }

        *data.progress[current_day].entry(username).or_insert(0) += count;
    }

    pub fn init_next_day(&self) {
        self.data.lock().unwrap().current_day += 1;
        self.data.lock().unwrap().progress.push(HashMap::new());
    }

    pub fn is_workout_over(&self) -> bool {
        let data = self.data.lock().unwrap();
        data.current_day + 1 >= data.duration
    }

    pub fn generate_daily_message(&self) -> String {
        let mut text = "".to_string();
        let data = self.data.try_lock().unwrap();

        let current_day = data.current_day;
        let duration = data.duration;
        let users = &data.users.clone();
        let progress= &data.progress;

        for username in users {
            text += &format!(
                "{}: {}\n",
                username,
                progress[current_day].get(username).unwrap_or(&0),
            );
        }

        text += &format!("День {}/{}\n", current_day + 1, duration);

        text
    }

    pub fn generate_final_message(&self) -> String {
        let mut users_progress = HashMap::new();
        let mut total_progress = 0;
        let data = self.data.lock().unwrap();

        for day_progress in &data.progress {
            for (username, count) in day_progress.into_iter() {
                *users_progress.entry(username).or_insert(0) += count;
                total_progress += count;
            }
        }

        let mut text = "".to_string();
        text += &format!(
            "Тренировка окончена! Мы прозанимались {} дней и отжались {} раз на всех.\n",
            data.duration,
            total_progress
        );

        for (username, count) in users_progress.into_iter() {
            text += &format!("{}: {}", username, count);
        }

        text
    }

    pub fn send_message(&self, text: String) -> Option<Message> {
        let send_message_params: SendMessageParams = SendMessageParamsBuilder::default()
            .chat_id(CHAT_ID)
            .text(text)
            .build()
            .unwrap();

        return match self.data.lock().unwrap().api.as_ref().unwrap().send_message(&send_message_params) {
            Ok(response) => {
                Some(response.result)
            }
            Err(err) => {
                println!("Failed to send message: {:?}", err);
                None
            }
        }
    }

    pub fn pin_daily_message(&self) {
        let daily_message_id = self.data.lock().unwrap().daily_message_id.clone();
        if let Some(daily_message_id) = daily_message_id {
            let pin_message_params: PinChatMessageParams = PinChatMessageParamsBuilder::default()
                .chat_id(CHAT_ID)
                .message_id(daily_message_id)
                .build()
                .unwrap();

            let result = self.data.lock().unwrap().api.as_ref().unwrap().pin_chat_message(&pin_message_params);

            if let Err(err) = result {
                println!("Error pining daily message: {:?}", err);
            }
        }
    }

    pub fn unpin_daily_message(&self) {
        let daily_message_id = self.data.lock().unwrap().daily_message_id.clone();
        if let Some(daily_message_id) = daily_message_id {
            let unpin_message_params: UnpinChatMessageParams = UnpinChatMessageParamsBuilder::default()
                .chat_id(CHAT_ID)
                .message_id(daily_message_id)
                .build()
                .unwrap();

            let result = self.data.lock().unwrap().api.as_ref().unwrap().unpin_chat_message(&unpin_message_params);

            if let Err(err) = result {
                println!("Error unpining daily message: {:?}", err);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let context = Arc::new(Context::default());
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(&token);

    context.data.lock().unwrap().api = Some(api);

    let cloned_context = Arc::clone(&context);
    let updates_handler =
        tokio::spawn(async move { get_updates(cloned_context).await });

    let cloned_context = Arc::clone(&context);
    let daily_message_sender =
        tokio::spawn(async move { send_daily_message(cloned_context).await });

    tokio::try_join!(updates_handler, daily_message_sender).unwrap();
}

async fn send_daily_message(context: Arc<Context>) {
    loop {
        context.unpin_daily_message();

        let text = context.generate_daily_message();

        if let Some(message) = context.send_message(text) {
            context.data.lock().unwrap().daily_message_id = Some(message.message_id);
            context.pin_daily_message();
        }

        time::sleep(get_day_duration()).await;

        if context.is_workout_over() {
            context.send_message(
                context.generate_final_message()
            );

            return;
        }

        context.init_next_day();
    }
}

fn update_daily_message(context: &Arc<Context>) -> Result<EditMessageResponse, frankenstein::Error> {
    let text = context.generate_daily_message();

    let update_message_params: EditMessageTextParams = EditMessageTextParamsBuilder::default()
        .chat_id(CHAT_ID)
        .message_id(context.data.lock().unwrap().daily_message_id.unwrap())
        .text(text)
        .build()
        .unwrap();

    context.data.lock().unwrap().api.as_ref().unwrap().edit_message_text(&update_message_params)
}

fn get_day_duration() -> core::time::Duration {
    // return Duration::seconds(5).to_std().unwrap();
    let now = Utc::now();
    let tomorrow_midnight = (now + Duration::days(1)).date().and_hms(0, 0, 0);

    tomorrow_midnight
        .signed_duration_since(now)
        .to_std()
        .unwrap()
}

async fn get_updates(context: Arc<Context>) {
    let token = env::var("TELEGRAM_BOT_TOKEN").expect("TELEGRAM_BOT_TOKEN not set");
    let api = Api::new(&token);

    let mut update_params: GetUpdatesParams = GetUpdatesParamsBuilder::default().build().unwrap();
    update_params.allowed_updates = Some(vec!["message".to_string()]);
    let update_delay = Duration::seconds(1).to_std().unwrap();

    loop {
        time::sleep(update_delay).await;
        let result = api.get_updates(&update_params);

        println!("result: {:?}", result);

        match result {
            Ok(response) => {
                for update in response.result {
                    update_params.offset = Some(update.update_id + 1);

                    if let Some(message) = update.message {
                        if message.chat.id == CHAT_ID {
                            process_message(message, &context);
                        }
                    }
                }
            }
            Err(error) => {
                println!("Failed to get updates: {:?}", error);
            }
        }
    }
}

fn process_message(message: Message, context: &Arc<Context>) {
    if message.text.is_none() {
        return;
    }

    let text = message.text.unwrap();
    let count = text.parse::<usize>();

    if count.is_err() {
        println!("{:?}", count);
        return;
    }

    let count = count.unwrap();

    let username = message.from.unwrap().username.unwrap();

    context.add_user_progress(username.clone(), count);

    match update_daily_message(context) {
        Ok(response) => println!("Edit ok: {:?}", response),
        Err(err) => println!("Failed to update daily message: {:?}", err),
    }

    if context.is_user_done(username) {
        context.send_message("🥳".to_string());
    }

    if context.is_all_users_done() {
        context.send_message("На сегодня всё 🎉".to_string());
    }
}