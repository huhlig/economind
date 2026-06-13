//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

use chrono::NaiveDateTime;

#[derive(Debug, Clone)]
pub struct Pivot {
    pub index: usize,
    pub timestamp: NaiveDateTime,
    pub price: f64,
    pub pivot_type: PivotType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PivotType {
    High,
    Low,
}
