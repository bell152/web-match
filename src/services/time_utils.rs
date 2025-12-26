use chrono::{DateTime, Utc, Timelike};

pub fn get_kline_start_time(dt: DateTime<Utc>, interval: &str) -> DateTime<Utc> {
    match interval {
        "1m" => dt.with_second(0).unwrap().with_nanosecond(0).unwrap(),
        "5m" => {
            let minute = dt.minute() - (dt.minute() % 5);
            dt.with_minute(minute).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        },
        "15m" => {
            let minute = dt.minute() - (dt.minute() % 15);
            dt.with_minute(minute).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        },
        "1h" => dt.with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap(),
        "4h" => {
            let hour = dt.hour() - (dt.hour() % 4);
            dt.with_hour(hour).unwrap().with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap()
        },
        "1d" => dt.with_hour(0).unwrap().with_minute(0).unwrap().with_second(0).unwrap().with_nanosecond(0).unwrap(),
        _ => dt, // Default fallback
    }
}
