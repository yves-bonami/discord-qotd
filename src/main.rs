mod bot;

use chrono::NaiveTime;
use structopt::StructOpt;
use tracing::info;

use crate::bot::{Bot, Webhook};

type Err = Box<dyn std::error::Error + Send + Sync + 'static>;

#[derive(Debug, StructOpt)]
struct Args {
    #[structopt(short = "c", long = "code", env = "QOTD_PASTEBIN")]
    code: String,
    #[structopt(short = "i", long = "id", env = "QOTD_WEBHOOK_ID")]
    webhook_id: u64,
    #[structopt(short = "t", long = "token", env = "QOTD_WEBHOOK_TOKEN")]
    webhook_token: String,
    #[structopt(long = "post_at", env = "QOTD_POST_AT", default_value = "12:00:00")]
    post_at: NaiveTime,
}

#[paw::main]
#[tokio::main]
async fn main(args: Args) -> Result<(), Err> {
    tracing_subscriber::fmt()
        .with_env_filter("qotd=debug")
        .init();

    let mut bot = Bot::new(
        args.code,
        Webhook::new(args.webhook_id, args.webhook_token),
        args.post_at,
    );

    tokio::select! {
        err = bot.start() => {
            info!("Bot stopped: {:?}", err);
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C received, stopping bot");
        },
    }

    Ok(())
}
