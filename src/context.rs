use frankenstein::{
    Api, EditMessageResponse, EditMessageTextParams, EditMessageTextParamsBuilder, Error, Message,
    PinChatMessageParams, PinChatMessageParamsBuilder, SendMessageParams, SendMessageParamsBuilder,
    TelegramApi, UnpinChatMessageParams, UnpinChatMessageParamsBuilder,
};
use std::collections::HashMap;
use tokio::sync::mpsc::Sender;

#[derive(Debug, Clone)]
pub enum ContextState {
    Created,
    Active,
}

pub enum ContextCommand {
    SendDailyMessage,
    AddPushups { username: String, count: usize },
}

#[derive(Debug)]
pub struct ContextData {
    pub chat_id: i64,
    pub daily_message_id: Option<i32>,
    pub current_day: usize,
    pub cycle_length: usize,
    pub cycle_increase: usize,
    pub duration: usize,
    pub repeats: usize,
    pub progress: Vec<HashMap<String, usize>>,
    pub users: Vec<String>,
    pub api: Api,
}

pub struct Contexts {
    pub api: Api,
    pub txs: HashMap<i64, Sender<ContextCommand>>,
}

impl Contexts {
    pub fn new(api: Api) -> Self {
        Self {
            api,
            txs: HashMap::new(),
        }
    }
}

impl ContextData {
    pub fn new(api: Api, chat_id: i64) -> Self {
        Self {
            api,
            chat_id,
            daily_message_id: None,
            cycle_increase: 10,
            cycle_length: 1,
            current_day: 0,
            progress: vec![HashMap::new()],
            duration: 3,
            repeats: 100,
            users: vec![],
        }
    }

    pub fn get_chat_id(&self) -> i64 {
        self.chat_id
    }

    pub fn is_user_done(&self, username: String) -> bool {
        self.progress[self.current_day].get(&username).unwrap_or(&0) >= &self.repeats
    }

    pub fn is_all_users_done(&self) -> bool {
        for username in &self.users {
            if !self.is_user_done(username.clone()) {
                return false;
            }
        }

        true
    }

    pub fn add_user_progress(&mut self, username: String, count: usize) {
        let current_day = self.current_day;

        if !self.users.contains(&username) {
            self.users.push(username.clone());
        }

        *self.progress[current_day].entry(username).or_insert(0) += count;
    }

    pub fn init_next_day(&mut self) -> bool {
        self.current_day += 1;
        self.progress.push(HashMap::new());

        if self.current_day != 1 && self.current_day % self.cycle_length == 0 {
            self.repeats += self.cycle_increase;

            return true;
        }

        false
    }

    pub fn is_workout_over(&self) -> bool {
        self.current_day >= self.duration
    }

    pub fn generate_daily_message(&self) -> String {
        let mut text = "".to_string();

        for username in &self.users {
            text += &format!(
                "{}: {}\n",
                username,
                self.progress[self.current_day].get(username).unwrap_or(&0),
            );
        }

        text += &format!(
            "День {} из {}. {} повторений\n",
            self.current_day, self.duration, self.repeats
        );

        text
    }

    pub fn generate_final_message(&self) -> String {
        let mut users_progress = HashMap::new();
        let mut total_progress = 0;

        for day_progress in &self.progress {
            for (username, count) in day_progress.into_iter() {
                *users_progress.entry(username).or_insert(0) += count;
                total_progress += count;
            }
        }

        let mut text = "".to_string();
        text += &format!(
            "Тренировка окончена! Мы прозанимались {} дней и отжались {} раз на всех.\n",
            self.duration, total_progress
        );

        for (username, count) in users_progress.into_iter() {
            text += &format!("{}: {}\n", username, count);
        }

        text
    }

    pub fn generate_end_of_cycle_message(&self) -> String {
        format!(
            "Очередной цикл завершён! Увеличиваем повторения с {} до {}.",
            self.repeats - self.cycle_increase,
            self.repeats
        )
    }

    pub fn send_message(&self, text: String) -> Option<Message> {
        let send_message_params: SendMessageParams = SendMessageParamsBuilder::default()
            .chat_id(self.chat_id)
            .text(text)
            .disable_notification(true)
            .build()
            .unwrap();

        return match self.api.send_message(&send_message_params) {
            Ok(response) => Some(response.result),
            Err(err) => {
                println!("Failed to send message: {:?}", err);
                None
            }
        };
    }

    pub fn pin_daily_message(&self) {
        if let Some(daily_message_id) = self.daily_message_id {
            let pin_message_params: PinChatMessageParams = PinChatMessageParamsBuilder::default()
                .chat_id(self.chat_id)
                .message_id(daily_message_id)
                .disable_notification(true)
                .build()
                .unwrap();

            let result = self.api.pin_chat_message(&pin_message_params);

            if let Err(err) = result {
                println!("Error pining daily message: {:?}", err);
            }
        }
    }

    pub fn unpin_daily_message(&self) {
        if let Some(daily_message_id) = self.daily_message_id {
            let unpin_message_params: UnpinChatMessageParams =
                UnpinChatMessageParamsBuilder::default()
                    .chat_id(self.chat_id)
                    .message_id(daily_message_id)
                    .build()
                    .unwrap();

            let result = self.api.unpin_chat_message(&unpin_message_params);

            if let Err(err) = result {
                println!("Error unpining daily message: {:?}", err);
            }
        }
    }

    pub fn update_daily_message(&self) -> Result<EditMessageResponse, frankenstein::Error> {
        if self.daily_message_id.is_none() {
            return Err(Error::DecodeError("No daily message ID".to_string()));
        }

        let text = self.generate_daily_message();

        let update_message_params: EditMessageTextParams = EditMessageTextParamsBuilder::default()
            .chat_id(self.chat_id)
            .message_id(self.daily_message_id.unwrap())
            .text(text)
            .build()
            .unwrap();

        self.api.edit_message_text(&update_message_params)
    }
}
