//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use super::types::Symbol;
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub enum NewsAbout {
    Symbol(Symbol),
    Industry(String),
    Sector(String),
}

#[derive(Debug, Clone)]
pub struct NewsStory {
    pub about: NewsAbout,
    pub headline: String,
    pub summary: String,
    pub story: String,
    pub url: String,
    pub evaluation: String,
    pub published_at: NaiveDate,
    pub fetched_at: NaiveDate,
}

#[derive(Debug, Clone)]
pub struct IncomeStatement {
    pub symbol: Symbol,
    pub period_end: NaiveDate,
    pub revenue: Decimal,
    pub cogs: Decimal,
    pub operating_income: Decimal,
    pub ebit: Decimal,
    pub net_income: Decimal,
    pub eps: Decimal,
    pub interest_expense: Decimal,
    pub tax_expense: Decimal,
}

#[derive(Debug, Clone)]
pub struct BalanceSheet {
    pub symbol: Symbol,
    pub period_end: NaiveDate,
    pub total_assets: Decimal,
    pub total_debt: Decimal,
    pub total_equity: Decimal,
    pub cash: Decimal,
}

#[derive(Debug, Clone)]
pub struct CashFlowStatement {
    pub symbol: Symbol,
    pub period_end: NaiveDate,
    pub operating_cash_flow: Decimal,
    pub capex: Decimal,
}

#[derive(Debug, Clone)]
pub struct DividendEvent {
    pub symbol: Symbol,
    pub ex_date: NaiveDate,
    pub payment_date: NaiveDate,
    pub amount: Decimal,
}

#[derive(Debug, Clone)]
pub struct StockSplitEvent {
    pub symbol: Symbol,
    pub date: NaiveDate,
    pub ratio: Decimal,
}
