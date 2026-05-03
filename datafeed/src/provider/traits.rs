//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use crate::ProviderResult;
use chrono::{NaiveDate, NaiveDateTime};
use economind_datamodel::model::{
    BalanceSheet, CandleEntry, CashFlowStatement, DailyCandleEntry, IncomeStatement, Interval,
    NewsStory, Symbol, TradeTick,
};
use futures_core::Stream;
use std::ops::Range;

pub trait FundamentalsProvider {
    async fn income_statements(&self, symbol: &str) -> ProviderResult<Vec<IncomeStatement>>;
    async fn balance_sheets(&self, symbol: &str) -> ProviderResult<Vec<BalanceSheet>>;
    async fn cash_flows(&self, symbol: &str) -> ProviderResult<Vec<CashFlowStatement>>;
}

pub trait DailyDataProvider: Send + Sync {
    async fn daily_bars(
        &self,
        symbol: &Symbol,
        date_range: Range<NaiveDate>,
    ) -> ProviderResult<Vec<DailyCandleEntry>>;
}

pub trait IntradayDataProvider: Send + Sync {
    async fn intraday_bars(
        &self,
        symbol: &Symbol,
        interval: Interval,
        time_range: Range<NaiveDateTime>,
    ) -> ProviderResult<Vec<CandleEntry>>;
}

pub trait TickDataProvider: Send + Sync {
    async fn trade_ticks(
        &self,
        symbol: &Symbol,
        time_range: Range<NaiveDateTime>,
    ) -> ProviderResult<Vec<TradeTick>>;
}

pub trait NewsProvider: Send + Sync {
    async fn news(
        &self,
        symbol: Option<&Symbol>,
        time_range: Option<Range<NaiveDateTime>>,
    ) -> ProviderResult<Vec<NewsStory>>;
}

pub trait StreamingMarketDataProvider: Send + Sync {
    type CandleStream: Stream<Item = ProviderResult<CandleEntry>> + Send + Unpin;
    type TickStream: Stream<Item = ProviderResult<TradeTick>> + Send + Unpin;

    async fn subscribe_bars(
        &self,
        symbol: Symbol,
        interval: Interval,
    ) -> ProviderResult<Self::CandleStream>;

    async fn subscribe_ticks(&self, symbol: Symbol) -> ProviderResult<Self::TickStream>;
}
