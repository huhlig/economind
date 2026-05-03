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
use chrono::{NaiveDate, NaiveDateTime};
use futures_core::stream::BoxStream;
use rust_decimal::Decimal;
use std::ops::Range;

pub struct TickerQuery {
    clauses: Vec<TickerQueryClause>,
}

pub enum TickerQueryClause {
    Symbol(Symbol),
    Exchange(String),
    AveragePrice(Decimal),
    AveragePriceDelta(Decimal),
    AverageVolume(Decimal),
}

pub trait MetadataStorage: Send + Sync {
    /// List all symbols
    async fn list_tickers(&self) -> StorageResult<BoxStream<'static, Symbol>>;

    /// Query intraday bars
    async fn query_tickers<'a>(
        &'a self,
        query: TickerQuery,
    ) -> StorageResult<BoxStream<'a, Ticker>>;

    /// Retrieve a single ticker by its symbol.
    async fn get_ticker(&self, symbol: &Symbol) -> StorageResult<Option<Ticker>>;

    /// Insert or update a ticker in the database.
    async fn upsert_ticker(&self, ticker: &Symbol) -> StorageResult<()>;

    async fn insert_news(&self, items: &[NewsStory]) -> StorageResult<()>;

    async fn query_news<'a>(
        &'a self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<BoxStream<'a, NewsStory>>;

    async fn insert_income_statements(&self, items: &[IncomeStatement]) -> StorageResult<()>;
    async fn query_income_statements<'a>(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<IncomeStatement>>;

    async fn insert_balance_sheets(&self, items: &[BalanceSheet]) -> StorageResult<()>;
    async fn query_balance_sheets<'a>(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<BalanceSheet>>;

    async fn insert_cash_flow_statements(&self, items: &[CashFlowStatement]) -> StorageResult<()>;
    async fn query_cash_flow_statements<'a>(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<CashFlowStatement>>;

    async fn insert_dividend_report(&self, items: &[DividendEvent]) -> StorageResult<()>;
    async fn query_dividend_report<'a>(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<DividendEvent>>;

    async fn insert_stock_split(&self, items: &[StockSplitEvent]) -> StorageResult<()>;
    async fn query_stock_split<'a>(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDate>>,
    ) -> StorageResult<Vec<StockSplitEvent>>;
}

pub trait CandleStorage: Send + Sync {
    /// Insert intraday bars (bulk optimized)
    async fn upsert_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        bars: &[CandleEntry],
    ) -> StorageResult<()>;

    /// Query intraday bars
    async fn query_candles(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<CandleEntry>>;

    /// Insert daily bars
    async fn unsert_daily_candle(
        &self,
        symbol: &Symbol,
        bars: &[DailyCandleEntry],
    ) -> StorageResult<()>;

    /// Query daily bars
    async fn query_daily_candles<'a>(
        &'a self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> StorageResult<BoxStream<'a, DailyCandleEntry>>;
}

pub trait TickStorage: Send + Sync {
    async fn insert_ticks(&self, ticks: &[TradeTick]) -> StorageResult<()>;

    async fn query_ticks(
        &self,
        symbol: &Symbol,
        time_range: Range<NaiveDateTime>,
    ) -> StorageResult<Vec<TradeTick>>;
}

pub trait StatisticsStorage: Send + Sync {
    async fn query_statistics(&self) -> StorageResult<Self>
    where
        Self: Sized;
}
