use builder_pattern::Builder;

/// Configure the batch copier actor: capacity, timing, and connection pool settings.
#[derive(Builder)]
pub struct Configuration {
    pub database_url: String,

    /// the actor's internal buffer
    #[default(8000)]
    pub max_rows_per_batch: usize,

    /// the mspc channel's buffer (fills while actor is busy with postgres io)
    #[default(8000)]
    pub max_channel_capacity: usize,

    /// send a flush message every _ ms. will be ignored if no rows in buffer
    #[default(500)]
    pub flush_timer_ms: u64,

    /// pool size, min 2 for failover purposes
    #[default(2)]
    pub pool_max_size: u32,

    /// duration to hold onto connection, recycle them ocassionally
    #[default(1200)]
    pub pool_max_lifetime_sec: u64,

    /// duration to wait for a connection, seconds
    #[default(2)]
    pub pool_connect_timeout_sec: u64,
}
