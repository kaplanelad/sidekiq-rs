use async_trait::async_trait;
use bb8_redis::{bb8::Pool, redis::AsyncCommands, RedisConnectionManager};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sidekiq::{ChainIter, Job, Processor, ServerMiddleware, ServerResult, Worker};
use slog::{error, info, o, Drain};

#[derive(Clone)]
struct HelloWorker {
    logger: slog::Logger,
}

impl HelloWorker {
    fn new(logger: slog::Logger) -> Self {
        Self { logger }
    }
}

#[async_trait]
impl Worker for HelloWorker {
    async fn perform(&self, _args: JsonValue) -> Result<(), Box<dyn std::error::Error>> {
        // I don't use any args. I do my own work.
        Ok(())
    }
}

#[derive(Clone)]
struct PaymentReportWorker {
    logger: slog::Logger,
}

impl PaymentReportWorker {
    fn new(logger: slog::Logger) -> Self {
        Self { logger }
    }

    async fn send_report(&self, user_guid: String) -> Result<(), Box<dyn std::error::Error>> {
        // TODO: Some actual work goes here...
        info!(self.logger, "Sending payment report to user"; "user_guid" => user_guid);

        Ok(())
    }
}

#[derive(Deserialize, Debug)]
struct PaymentReportArgs {
    user_guid: String,
}

#[async_trait]
impl Worker for PaymentReportWorker {
    async fn perform(&self, args: JsonValue) -> Result<(), Box<dyn std::error::Error>> {
        // I use serde to pull out my args as a type. I fail if the value cannot be decoded.
        // NOTE: I use a size-one (tuple,) tuple because args are a JsonArray.
        let (args,): (PaymentReportArgs,) = serde_json::from_value(args)?;
        Ok(())
    }
}

struct FilterExpiredUsersMiddleware {
    logger: slog::Logger,
}

impl FilterExpiredUsersMiddleware {
    fn new(logger: slog::Logger) -> Self {
        Self { logger }
    }
}

#[derive(Deserialize)]
struct FiltereExpiredUsersArgs {
    user_guid: String,
}

impl FiltereExpiredUsersArgs {
    fn is_expired(&self) -> bool {
        self.user_guid == "USR-123-EXPIRED"
    }
}

#[async_trait]
impl ServerMiddleware for FilterExpiredUsersMiddleware {
    async fn call(
        &self,
        chain: ChainIter,
        job: Job,
        worker: Box<dyn Worker>,
        mut redis: Pool<RedisConnectionManager>,
    ) -> ServerResult {
        let args: Option<(FiltereExpiredUsersArgs,)> = serde_json::from_value(job.args.clone())?;

        if let Some((filter,)) = args {
            if filter.is_expired() {
                error!(
                    self.logger,
                    "Detected an expired user, skipping this job";
                    "class" => job.class,
                    "jid" => job.jid,
                    "user_guid" => filter.user_guid,
                );
                return Ok(());
            }
        }

        chain.next(job, worker, redis).await
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logger
    let decorator = slog_term::PlainSyncDecorator::new(std::io::stdout());
    let drain = slog_term::FullFormat::new(decorator).build().fuse();
    let logger = slog::Logger::root(drain, o!());

    // Redis
    let manager = RedisConnectionManager::new("redis://127.0.0.1/").unwrap();
    let redis = Pool::builder().build(manager).await.unwrap();

    // Sidekiq
    let mut p = Processor::new(redis, logger.clone(), vec!["queue:yolo".to_string()]);

    // Add known workers
    p.register("HelloWorker", Box::new(HelloWorker::new(logger.clone())));
    p.register(
        "PaymentReportWorker",
        Box::new(PaymentReportWorker::new(logger.clone())),
    );

    // Custom Middlewares
    p.using(Box::new(FilterExpiredUsersMiddleware::new(logger.clone())))
        .await;

    p.run().await;
    Ok(())
}