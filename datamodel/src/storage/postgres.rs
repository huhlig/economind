//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use sqlx::{PgPool, Row};
use sqlx::postgres::PgRow;
use crate::model::{Exchange, Symbol, Ticker};
use crate::{StorageError, StorageResult};

/// DataManager is responsible for handling all database operations for Economind.
pub struct PostgresStorage {
    database: PgPool,
}

impl PostgresStorage {
    /// Create a new DataManager instance with a given PostgreSQL pool.
    pub fn new(pool: PgPool) -> Self {
        Self { database: pool }
    }

    /// Retrieve a list of all tickers from the database.
    pub async fn list_tickers(&self) -> StorageResult<Vec<Ticker>> {
        Ok(sqlx::query("SELECT symbol, exchange, name, country, industry, sector, ipoyear, marketcap, description FROM economind.tickers")
            .map(|row: PgRow| Ticker {
                symbol: Symbol::new(row.get("symbol")),
                exchange: Exchange::new(row.get("exchange")),
                name: row.get("name"),
                country: row.get("country"),
                industry: row.get("industry"),
                sector: row.get("sector"),
                ipoyear: row.get("ipoyear"),
                marketcap: row.get("marketcap"),
                description: row.get("description"),
            })
            .fetch_all(&self.database)
            .await.map_err(|err| StorageError::DatabaseError(err))?)
    }

    /// Retrieve a single ticker by its symbol.
    pub async fn get_ticker(&self, symbol: &str) -> StorageResult<Option<Ticker>> {
        Ok(sqlx::query("SELECT symbol, exchange, name, marketcap, country, ipoyear, industry, sector, description FROM economind.tickers WHERE symbol = $1")
            .bind(symbol)
            .map(|row: PgRow| Ticker {
                symbol: Symbol::new(row.get("symbol")),
                exchange: Exchange::new(row.get("exchange")),
                name: row.get("name"),
                country: row.get("country"),
                industry: row.get("industry"),
                sector: row.get("sector"),
                ipoyear: row.get("ipoyear"),
                marketcap: row.get("marketcap"),
                description: row.get("description"),
            })
            .fetch_optional(&self.database)
            .await.map_err(|err| StorageError::DatabaseError(err))?)
    }

    /// Insert or update a ticker in the database.
    pub async fn upsert_ticker(&self, ticker: &Ticker) -> StorageResult<()> {
        sqlx::query("INSERT INTO economind.tickers (symbol, exchange, name, country, industry, sector, ipoyear, marketcap, description) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) ON CONFLICT (symbol) DO UPDATE SET exchange = EXCLUDED.exchange, name = EXCLUDED.name, marketcap = EXCLUDED.marketcap, country = EXCLUDED.country, ipoyear = EXCLUDED.ipoyear, industry = EXCLUDED.industry, sector = EXCLUDED.sector, description = EXCLUDED.description;")
            .bind(&ticker.symbol.as_str())
            .bind(&ticker.exchange.as_str())
            .bind(&ticker.name)
            .bind(&ticker.country)
            .bind(&ticker.industry)
            .bind(&ticker.sector)
            .bind(&ticker.ipoyear)
            .bind(&ticker.marketcap)
            .bind(&ticker.description)
            .execute(&self.database)
            .await.map_err(|err| StorageError::DatabaseError(err))?;
        Ok(())
    }

    /// Retrieve statistics for a single ticker by its symbol.
    pub async fn get_ticker_stats(&self, symbol: &str) -> StorageResult<Option<TickerStats>> {
        Ok(sqlx::query("SELECT symbol, lastsale, netchange, pctchange, volume, start_date, end_date FROM economind.ticker_stats WHERE symbol = $1")
            .bind(symbol)
            .map(|row: PgRow| TickerStats {
                symbol: row.get("symbol"),
                lastsale: row.get("lastsale"),
                netchange: row.get("netchange"),
                pctchange: row.get("pctchange"),
                volume: row.get("volume"),
                start_date: row.get("start_date"),
                end_date: row.get("end_date"),
            })
            .fetch_optional(&self.database)
            .await.map_err(|err| StorageError::DatabaseError(err))?)
    }

    /// Insert or update ticker statistics in the database.
    pub async fn upsert_ticker_stats(&self, ticker_stats: &TickerStats) -> DataResult<()> {
        sqlx::query("INSERT INTO economind.ticker_stats (symbol, lastsale, netchange, pctchange, volume, start_date, end_date) VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT (symbol) DO UPDATE SET lastsale = EXCLUDED.lastsale, netchange = EXCLUDED.netchange, pctchange = EXCLUDED.pctchange, volume = EXCLUDED.volume, start_date = EXCLUDED.start_date, end_date = EXCLUDED.end_date;")
            .bind(&ticker_stats.symbol)
            .bind(&ticker_stats.lastsale)
            .bind(&ticker_stats.netchange)
            .bind(&ticker_stats.pctchange)
            .bind(ticker_stats.volume)
            .bind(ticker_stats.start_date)
            .bind(ticker_stats.end_date)
            .execute(&self.database)
            .await.map_err(|err| DataError::DatabaseError(err))?;
        Ok(())
    }

    /// Retrieve a series of daily candles for a symbol, optionally within a date range.
    /// Returns a `DailyCandleSeries` containing the aggregated stats and individual candles.
    pub async fn get_daily_candle(
        &self,
        symbol: &str,
        start: Option<NaiveDate>,
        end: Option<NaiveDate>,
    ) -> DataResult<Option<DailyCandleSeries>> {
        let mut sql = "SELECT date, open, high, low, close, volume FROM economind.daily_candle WHERE symbol = $1".to_string();
        let mut param_idx = 2;
        if start.is_some() {
            sql.push_str(&format!(" AND date >= ${}", param_idx));
            param_idx += 1;
        }
        if end.is_some() {
            sql.push_str(&format!(" AND date <= ${}", param_idx));
        }
        sql.push_str(" ORDER BY date ASC");

        let mut query = sqlx::query(&sql).bind(symbol);
        if let Some(start_date) = start {
            query = query.bind(start_date);
        }
        if let Some(end_date) = end {
            query = query.bind(end_date);
        }

        let rows = query
            .fetch_all(&self.database)
            .await
            .map_err(|err| DataError::DatabaseError(err))?;

        if rows.is_empty() {
            return Ok(None);
        }

        let mut entries: Vec<DailyCandle> = rows
            .iter()
            .map(|row| DailyCandle {
                date: row.get("date"),
                open: row.get("open"),
                high: row.get("high"),
                low: row.get("low"),
                close: row.get("close"),
                volume: row.get("volume"),
            })
            .collect();
        entries.sort_by_key(|c| c.date);

        let start_time = entries.first().unwrap().date;
        let end_time = entries.last().unwrap().date;
        let open = entries.first().unwrap().open as f64;
        let close = entries.last().unwrap().close as f64;
        let high = entries.iter().map(|c| c.high).fold(f32::MIN, f32::max) as f64;
        let low = entries.iter().map(|c| c.low).fold(f32::MAX, f32::min) as f64;
        let volume = entries.iter().map(|c| c.volume as i64).sum();

        Ok(Some(DailyCandleSeries {
            ticker: symbol.to_string(),
            start_time,
            end_time,
            open,
            high,
            low,
            close,
            volume,
            entries,
        }))
    }

    /// Insert or update multiple daily candles for a symbol.
    /// Uses PostgreSQL's `UNNEST` for efficient bulk upsert.
    #[rustfmt::skip]
    pub async fn upsert_daily_candle(&self, symbol: &str, daily_candles: &[DailyCandle]) -> DataResult<()> {
        let symbols = daily_candles.iter().map(|_| symbol.to_string()).collect::<Vec<String>>();
        let dates = daily_candles.iter().map(|candle| candle.date).collect::<Vec<NaiveDate>>();
        let opens = daily_candles.iter().map(|candle| candle.open).collect::<Vec<f32>>();
        let highs = daily_candles.iter().map(|candle| candle.high).collect::<Vec<f32>>();
        let lows = daily_candles.iter().map(|candle| candle.low).collect::<Vec<f32>>();
        let closes = daily_candles.iter().map(|candle| candle.close).collect::<Vec<f32>>();
        let volumes = daily_candles.iter().map(|candle| candle.volume).collect::<Vec<i32>>();
        sqlx::query("
INSERT INTO economind.daily_candle (symbol, date, open, high, low, close, volume)
SELECT * FROM UNNEST($1::VARCHAR[], $2::DATE[], $3::FLOAT[], $4::FLOAT[], $5::FLOAT[], $6::FLOAT[], $7::INTEGER[])
ON CONFLICT (symbol, date) DO UPDATE SET open = EXCLUDED.open, high = EXCLUDED.high,
low = EXCLUDED.low, close = EXCLUDED.close, volume = EXCLUDED.volume;")
            .bind(symbols)
            .bind(dates)
            .bind(opens)
            .bind(highs)
            .bind(lows)
            .bind(closes)
            .bind(volumes)
            .execute(&self.database)
            .await.map_err(|err| DataError::DatabaseError(err))?;
        Ok(())
    }
}

impl MetadataStorage for PostgresStorage {
    //
}