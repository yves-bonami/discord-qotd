use chrono::{NaiveTime, Timelike};
use isahc::prelude::*;
use rand::prelude::SliceRandom;
use serde::{Deserialize, Serialize};
use serenity::model::channel::Embed;
use std::fmt;
use tokio::{
    fs::OpenOptions,
    io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt},
    time,
};
use tracing::info;
use uuid::Uuid;

type Err = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Serialize, Deserialize)]
pub struct Bot {
    url: String,
    hook: Webhook,
    post_at: NaiveTime,
    pub questions: Vec<Question>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Question {
    id: Uuid,
    text: String,
    answered: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Webhook {
    id: u64,
    token: String,
}

impl Bot {
    pub fn new(url: String, hook: Webhook, post_at: NaiveTime) -> Self {
        Self {
            questions: vec![],
            url: format!("https://pastebin.com/raw/{}", url),
            hook,
            post_at,
        }
    }

    pub async fn start(&mut self) -> Result<(), Err> {
        let mut interval = time::interval(time::Duration::from_secs(60));
        loop {
            interval.tick().await;

            self.restore().await?;
            self.load().await?;

            let now = chrono::Utc::now();
            if self.questions.len() > 0
                && now.hour() == self.post_at.hour()
                && now.minute() == self.post_at.minute()
            {
                self.answer().await?;
            }

            self.save().await?;
        }
    }

    #[tracing::instrument]
    async fn restore(&mut self) -> Result<(), Err> {
        // Restore from file
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open("questions.json")
            .await?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).await?;
        self.questions = serde_json::from_str(&contents).unwrap_or_default();
        info!("Restored {} questions", self.questions.len());

        Ok(())
    }

    #[tracing::instrument]
    async fn save(&mut self) -> Result<(), Err> {
        // Save to file
        let mut file = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open("questions.json")
            .await?;
        let json = serde_json::to_string(&self.questions)?;
        file.set_len(0).await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        file.write_all(json.as_bytes()).await?;
        info!("Saved {} questions", self.questions.len());

        Ok(())
    }

    #[tracing::instrument]
    async fn load(&mut self) -> Result<(), Err> {
        let mut response = isahc::get_async(&self.url).await?;
        let raw = response.text().await?;
        let raw_questions = raw.split("\n");

        for question in raw_questions {
            match self.questions.iter_mut().find(|q| q.distance(question) < 4) {
                Some(q) => {
                    if q.distance(question) != 0 {
                        info!("Updating existing question {}", q.id);
                        q.text = question.trim().to_string()
                    }
                }
                None => {
                    let new_question = Question::new(question.trim().into());
                    info!("Adding new question {}", &new_question.id);
                    self.questions.push(new_question);
                }
            }
        }

        Ok(())
    }

    #[tracing::instrument]
    async fn answer(&mut self) -> Result<(), Err> {
        let mut unanswered_questions = self
            .questions
            .iter_mut()
            .filter(|q| !q.answered)
            .collect::<Vec<&mut Question>>();

        if unanswered_questions.len() == 0 {
            info!("No unanswered questions");
            return Ok(());
        }

        let mut rng = rand::thread_rng();
        unanswered_questions.shuffle(&mut rng);

        let question = unanswered_questions.first_mut().unwrap();
        info!("{}", question.text);

        self.hook.send(question.text.clone()).await?;

        question.answered = true;

        Ok(())
    }
}

impl fmt::Debug for Bot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bot {{ url: {} }}", self.url)
    }
}

impl Question {
    pub fn new(text: String) -> Question {
        Question {
            id: Uuid::new_v4(),
            text,
            answered: false,
        }
    }

    fn distance(&self, other: &str) -> usize {
        //Damerau-Levenshtein distance
        if self.text == other {
            return 0;
        }

        if self.text.len() == 0 {
            return other.len();
        }

        if other.len() == 0 {
            return self.text.len();
        }

        let mut matrix = vec![vec![0; other.len() + 1]; self.text.len() + 1];
        for i in 1..=self.text.len() {
            matrix[i][0] = i;
            for j in 1..=other.len() {
                let cost = if self.text.chars().nth(i - 1) == other.chars().nth(j - 1) {
                    0
                } else {
                    1
                };
                if i == 1 {
                    matrix[0][j] = j;
                }

                let vals = [
                    matrix[i - 1][j] + 1,
                    matrix[i][j - 1] + 1,
                    matrix[i - 1][j - 1] + cost,
                ];
                matrix[i][j] = *vals.iter().min().unwrap();
                if i > 1
                    && j > 1
                    && self.text.chars().nth(i - 1) == other.chars().nth(j - 2)
                    && self.text.chars().nth(i - 2) == other.chars().nth(j - 1)
                {
                    matrix[i][j] = std::cmp::min(matrix[i][j], matrix[i - 2][j - 2] + cost);
                }
            }
        }

        matrix[self.text.len()][other.len()]
    }
}

impl Webhook {
    pub fn new(id: u64, token: String) -> Self {
        Self { id, token }
    }

    async fn send(&self, text: String) -> Result<(), Err> {
        let http = serenity::http::Http::new_with_token(&self.token);
        let webhook = http.get_webhook_with_token(self.id, &self.token).await?;

        let embed = Embed::fake(|e| {
            e.title(":question: :grey_question: Question of the day :grey_question: :question:");
            e.description(text + "\n\u{200B}");
            e.colour(0xff0000);
            e.footer(|f| {
                f.text(format!(
                    "Asked by Hawk's bot at {}",
                    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S")
                ));
                f
            });
            e
        });

        webhook
            .execute(&http, false, |w| {
                w.username("Question of the day");
                w.embeds(vec![embed]);
                w
            })
            .await?;

        Ok(())
    }
}
