//
// Copyright (C) Hans W. Uhlig - All Rights Reserved
//
// This source code is protected under international copyright law. All rights reserved and
// protected by the copyright holders. This file is confidential and only available to authorized
// individuals with the permission of the copyright holders. If you encounter this file and do not
// have permission, please contact the copyright holders and delete this file.
//

#[derive(Debug, Clone)]
pub struct Trendline {
    pub slope: f64,
    pub intercept: f64,
    pub touches: usize,
    pub error: f64,
}
