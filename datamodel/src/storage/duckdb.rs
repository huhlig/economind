//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::StorageResult;
use crate::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, DividendEvent, IncomeStatement,
    Interval, NewsStory, StockSplitEvent, Symbol, Ticker, TradeTick,
};
use crate::storage::{
    CandleStorage, MetadataStorage, StatisticsStorage, TickStorage, TickerQuery,
};
use duckdb::Connection;
use chrono::{NaiveDate, NaiveDateTime};
use futures_core::stream::BoxStream;
use std::ops::Range;
use std::path::{Path, PathBuf};

pub struct DuckDatabase {
    path: PathBuf,
}

impl DuckDatabase {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        DuckDatabase {
            path: path.as_ref().to_path_buf(),
        }
    }

    fn connect(&self) -> StorageResult<Connection> {
        Connection::open(&self.path).map_err(|e| crate::StorageError::Provider(e.to_string()))
    }

    pub fn initialize(&self) -> StorageResult<()> {
        let conn = self.connect()?;
        conn.execute_batch(include_str!("schema.sql"))
            .map_err(|e| crate::StorageError::Provider(e.to_string()))?;

        Ok(())
    }
}

impl MetadataStorage for DuckDatabase {
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>> {
        todo!()
    }

    async fn query_tickers<'a>(
        &'a self,
        _query: TickerQuery,
    ) -> StorageResult<BoxStream<'a, Ticker>> {
        todo!()
    }

    async fn get_ticker(&self, _symbol: &Symbol) -> StorageResult<Option<Ticker>> {
        todo!()
    }

    async fn upsert_ticker(&self, _ticker: &Symbol) -> StorageResult<()> {
        todo!()
    }

    async fn insert_news(&self, _items: &[NewsStory]) -> StorageResult<()> {
        todo!()
    }

    async fn query_news<'a>(
        &'a self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<BoxStream<'a, NewsStory>> {
        todo!()
    }

    async fn insert_income_statements(&self, _items: &[IncomeStatement]) -> StorageResult<()> {
        todo!()
    }

    async fn query_income_statements<'a>(
        &self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<IncomeStatement>> {
        todo!()
    }

    async fn insert_balance_sheets(&self, _items: &[BalanceSheet]) -> StorageResult<()> {
        todo!()
    }

    async fn query_balance_sheets<'a>(
        &self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<BalanceSheet>> {
        todo!()
    }

    async fn insert_cash_flow_statements(&self, _items: &[CashFlowStatement]) -> StorageResult<()> {
        todo!()
    }

    async fn query_cash_flow_statements<'a>(
        &self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>> {
        todo!()
    }

    async fn insert_dividend_report(&self, _items: &[DividendEvent]) -> StorageResult<()> {
        todo!()
    }

    async fn query_dividend_report<'a>(
        &self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<DividendEvent>> {
        todo!()
    }

    async fn insert_stock_split(&self, _items: &[StockSplitEvent]) -> StorageResult<()> {
        todo!()
    }

    async fn query_stock_split<'a>(
        &self,
        _symbol: Option<&Symbol>,
        _time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<StockSplitEvent>> {
        todo!()
    }
}

impl StatisticsStorage for DuckDatabase {
    async fn query_statistics(&self) -> StorageResult<Self>
    where
        Self: Sized,
    {
        todo!()
    }
}

impl CandleStorage for DuckDatabase {
    async fn upsert_candles(
        &self,
        _symbol: &Symbol,
        _interval: Interval,
        _bars: &[CandleEntry],
    ) -> StorageResult<()> {
        todo!()
    }

    async fn query_candles(
        &self,
        _symbol: &Symbol,
        _interval: Interval,
        _time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<CandleEntry>> {
        todo!()
    }

    async fn unsert_daily_candle(
        &self,
        _symbol: &Symbol,
        _bars: &[DailyCandleEntry],
    ) -> StorageResult<()> {
        todo!()
    }

    async fn query_daily_candles<'a>(
        &'a self,
        _symbol: &Symbol,
        _date_range: Range<NaiveDate>,
    ) -> StorageResult<BoxStream<'a, DailyCandleEntry>> {
        todo!()
    }
}

impl TickStorage for DuckDatabase {
    async fn insert_ticks(&self, _ticks: &[TradeTick]) -> StorageResult<()> {
        todo!()
    }

    async fn query_ticks(
        &self,
        _symbol: &Symbol,
        _time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<TradeTick>> {
        todo!()
    }
}

