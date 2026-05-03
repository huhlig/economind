CREATE TABLE IF NOT EXISTS tickers
(
    symbol      TEXT PRIMARY KEY,
    exchange    TEXT,
    name        TEXT,
    country     TEXT,
    industry    TEXT,
    sector      TEXT,
    ipoyear     TEXT,
    marketcap   FLOAT,
    description TEXT
);
CREATE TABLE IF NOT EXISTS candles
(
    symbol    TEXT,
    interval  TEXT,
    timestamp TIMESTAMP,
    open      FLOAT,
    high      FLOAT,
    low       FLOAT,
    close     FLOAT,
    volume    INTEGER,
    PRIMARY KEY (symbol, interval, timestamp)
);
CREATE TABLE IF NOT EXISTS daily_candles
(
    symbol TEXT,
    date   DATE,
    open   FLOAT,
    high   FLOAT,
    low    FLOAT,
    close  FLOAT,
    volume FLOAT,
    PRIMARY KEY (symbol, date)
);
CREATE TABLE IF NOT EXISTS ticks
(
    symbol    TEXT,
    timestamp TIMESTAMP,
    price     FLOAT,
    size      INTEGER,
    PRIMARY KEY (symbol, timestamp)
);
CREATE TABLE IF NOT EXISTS news
(
    symbol       TEXT,
    headline     TEXT,
    summary      TEXT,
    story        TEXT,
    url          TEXT,
    evaluation   TEXT,
    published_at TIMESTAMP,
    fetched_at   TIMESTAMP
);