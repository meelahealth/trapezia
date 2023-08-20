use std::marker::PhantomData;

use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use deadpool_redis::{Config, Runtime};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use super::SessionId;
use crate::PREFIX;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session<D: Clone> {
    pub id: SessionId,
    pub data: D,
    pub expires_at: DateTime<Utc>,
}

// #[derive(Debug, Clone, Serialize, Deserialize)]
// pub struct SessionData<U> {
//     pub user_id: U,
// }

pub struct Backend<D: Clone> {
    pub(crate) pool: deadpool_redis::Pool,
    _data: PhantomData<D>,
}

impl<U: Clone> Backend<U> {
    pub fn new(url: &str) -> Result<Self, deadpool_redis::CreatePoolError> {
        let config = Config::from_url(url);
        let pool = config.create_pool(Some(Runtime::Tokio1))?;
        Ok(Self {
            pool,
            _data: PhantomData,
        })
    }

    pub fn with_pool(pool: deadpool_redis::Pool) -> Self {
        Self {
            pool,
            _data: PhantomData,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Error establishing connection to Redis pool")]
    Pool(#[from] deadpool_redis::PoolError),

    #[error("Redis error")]
    Redis(#[from] redis::RedisError),

    #[error("Json parsing error")]
    Json(#[from] serde_json::Error),

    #[error("Session not found for given id {0}")]
    NotFound(SessionId),

    #[error("An error occurred: {0}")]
    Custom(String),
}

#[async_trait]
impl<D> super::SessionBackend for Backend<D>
where
    D: Clone + Serialize + DeserializeOwned + Send + Sync,
{
    type Error = Error;
    type Session = Session<D>;
    type SessionData = D;

    async fn new_session(
        &self,
        data: Self::SessionData,
        expires_at: DateTime<Utc>,
    ) -> Result<Self::Session, Self::Error> {
        let mut conn = self.pool.get().await?;
        let session_id = SessionId::new();
        let session = Session {
            id: session_id,
            data,
            expires_at,
        };
        redis::cmd("SET")
            .arg(format!("{PREFIX}/session/{}", session_id))
            .arg(serde_json::to_string(&session.data).unwrap())
            .arg("EXAT")
            .arg(expires_at.timestamp())
            .query_async(&mut conn)
            .await?;
        Ok(session)
    }

    async fn session(
        &self,
        id: SessionId,
        extend_expiry: Option<DateTime<Utc>>,
    ) -> Result<Self::Session, Self::Error> {
        let mut conn = self.pool.get().await?;

        // TODO: handle NotFound properly. Right now it's hidden in a RedisError.

        let (session_data, ttl): (String, i64) = match extend_expiry {
            Some(expiry) => {
                redis::pipe()
                    .atomic()
                    .cmd("GETEX")
                    .arg(format!("{PREFIX}/session/{}", id))
                    .arg("EXAT")
                    .arg(expiry.timestamp())
                    .cmd("TTL")
                    .arg(format!("{PREFIX}/session/{}", id))
                    .query_async(&mut conn)
                    .await?
            }
            None => {
                redis::pipe()
                    .atomic()
                    .cmd("GET")
                    .arg(format!("{PREFIX}/session/{}", id))
                    .cmd("TTL")
                    .arg(format!("{PREFIX}/session/{}", id))
                    .query_async(&mut conn)
                    .await?
            }
        };

        let data = serde_json::from_str(&session_data)?;

        let session = Session {
            id,
            data,
            expires_at: Utc::now() + Duration::seconds(ttl),
        };

        Ok(session)
    }

    async fn clear_stale_sessions(&self) -> Result<(), Self::Error> {
        // Not really supported by Redis, does it itself.
        Ok(())
    }

    async fn expire(&self, session: Self::Session) -> Result<(), Self::Error> {
        let mut conn = self.pool.get().await?;
        redis::cmd("DEL")
            .arg(format!("{PREFIX}/session/{}", session.id))
            .query_async(&mut conn)
            .await?;
        Ok(())
    }
}
