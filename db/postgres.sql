--
-- PostgreSQL database setup script for Economind
--

BEGIN;

--
-- Extensions
--

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE EXTENSION IF NOT EXISTS hstore;

--
-- Schema
--

CREATE SCHEMA economind;

-- SET Search Path

SET search_path TO economind, public;

--
-- Name: settings; Type: TABLE; Schema: economind; Owner: economind
--

CREATE TABLE economind.settings
(
    key   VARCHAR(100) PRIMARY KEY,
    value TEXT
);

COMMENT ON TABLE economind.settings IS 'Economind Settings';
COMMENT ON COLUMN economind.settings.key IS 'Property Key (e.g., banner.welcome, banner.motd, account.creation_enabled)';
COMMENT ON COLUMN economind.settings.value IS 'Property Value (can be large text for banners)';

--
-- Name: tickers; Type: TABLE; Schema: economind; Owner: economind
--

CREATE TABLE economind.tickers
(
    symbol      VARCHAR(10) NOT NULL,
    exchange    VARCHAR(10),
    name        VARCHAR(500),
    country     VARCHAR(50),
    industry    VARCHAR(200),
    sector      VARCHAR(200),
    ipoyear     VARCHAR(5),
    marketcap   FLOAT,
    description TEXT,
    PRIMARY KEY (symbol)
);

COMMENT ON TABLE economind.tickers IS 'Economind Settings';
COMMENT ON COLUMN economind.tickers.symbol IS 'Ticker Symbol';
COMMENT ON COLUMN economind.tickers.exchange IS 'Ticker Symbol';
COMMENT ON COLUMN economind.tickers.name IS 'Instrument name';
COMMENT ON COLUMN economind.tickers.country IS 'Country';
COMMENT ON COLUMN economind.tickers.industry IS 'Industry';
COMMENT ON COLUMN economind.tickers.sector IS 'Sector';
COMMENT ON COLUMN economind.tickers.ipoyear IS 'Year of Initial Public Offering';
COMMENT ON COLUMN economind.tickers.marketcap IS 'Market Cap';
COMMENT ON COLUMN economind.tickers.description IS 'Description';


--
-- Name: tickers; Type: TABLE; Schema: economind; Owner: economind
--

CREATE TABLE economind.ticker_stats
(
    symbol     VARCHAR(10) NOT NULL,
    lastsale   FLOAT,
    netchange  FLOAT,
    pctchange  FLOAT,
    volume     INTEGER,
    start_date DATE,
    end_date   DATE,
    PRIMARY KEY (symbol)
);

COMMENT ON TABLE economind.ticker_stats IS 'Economind Settings';
COMMENT ON COLUMN economind.ticker_stats.symbol IS 'Ticker Symbol';
COMMENT ON COLUMN economind.ticker_stats.lastsale IS 'Last Sale Value';
COMMENT ON COLUMN economind.ticker_stats.netchange IS 'Net Change';
COMMENT ON COLUMN economind.ticker_stats.pctchange IS 'Pct Change';
COMMENT ON COLUMN economind.ticker_stats.volume IS 'Trading Volume';

--
-- Name: candle; Type: TABLE; Schema: economind; Owner: economind
--

CREATE TABLE economind.daily_candle
(
    symbol VARCHAR(10) NOT NULL,
    date   DATE,
    open   FLOAT,
    high   FLOAT,
    low    FLOAT,
    close  FLOAT,
    volume INTEGER,
    PRIMARY KEY (symbol, date)
);

COMMENT ON TABLE economind.daily_candle IS 'Economind Settings';
COMMENT ON COLUMN economind.daily_candle.symbol IS 'Ticker Symbol';
COMMENT ON COLUMN economind.daily_candle.date IS 'Candle Date';
COMMENT ON COLUMN economind.daily_candle.open IS 'Candle Open';
COMMENT ON COLUMN economind.daily_candle.high IS 'Candle High';
COMMENT ON COLUMN economind.daily_candle.low IS 'Candle Low';
COMMENT ON COLUMN economind.daily_candle.close IS 'Candle Close';
COMMENT ON COLUMN economind.daily_candle.volume IS 'Volume';

COMMIT;