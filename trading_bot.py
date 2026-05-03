#!/usr/bin/env python3
"""
═══════════════════════════════════════════════════════════════
  LOW-FREQUENCY SWING TRADE SCANNER
  1–10 Day Holding Period | Yahoo Finance | Claude AI News Sentiment
═══════════════════════════════════════════════════════════════

FEATURES:
  • RSI, MACD, Bollinger Bands technical analysis
  • Volume & momentum signals
  • Earnings catalyst detection
  • AI-powered news sentiment (via Claude API)
  • Composite scoring with BUY / WATCH / AVOID ratings
  • HTML report output

SETUP:
  pip install yfinance pandas numpy requests beautifulsoup4 anthropic

OPTIONAL (for AI news sentiment):
  export ANTHROPIC_API_KEY="your-key-here"

USAGE:
  python trading_bot.py                    # Scan default watchlist
  python trading_bot.py --tickers AAPL MSFT NVDA   # Custom tickers
  python trading_bot.py --sp500            # Scan full S&P 500
  python trading_bot.py --report           # Save HTML report
"""

import argparse
import json
import os
import sys
import time
import warnings
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Optional

warnings.filterwarnings("ignore")

# ─── Third-party imports (install via pip) ───────────────────────────────────
try:
    import yfinance as yf
except ImportError:
    sys.exit("❌  Missing: pip install yfinance")

try:
    import numpy as np
    import pandas as pd
except ImportError:
    sys.exit("❌  Missing: pip install pandas numpy")

try:
    import requests
    from bs4 import BeautifulSoup
except ImportError:
    sys.exit("❌  Missing: pip install requests beautifulsoup4")

# Claude is optional — gracefully degrade if not installed / no API key
try:
    import anthropic
    CLAUDE_AVAILABLE = bool(os.environ.get("ANTHROPIC_API_KEY"))
except ImportError:
    CLAUDE_AVAILABLE = False


# ═══════════════════════════════════════════════════════════════
#  CONFIG — tweak these to your preference
# ═══════════════════════════════════════════════════════════════

DEFAULT_WATCHLIST = [
    # Large-cap tech momentum names
    "AAPL", "MSFT", "NVDA", "AMD", "META", "GOOGL", "AMZN",
    # High-beta / swing-friendly
    "TSLA", "PLTR", "MSTR", "COIN", "SOFI",
    # Financials & cyclicals
    "JPM", "BAC", "GS", "XOM", "CVX",
    # ETFs for sector context
    "SPY", "QQQ", "IWM",
]

# Scoring weights (must sum to 1.0)
WEIGHTS = {
    "rsi":        0.15,
    "macd":       0.15,
    "bollinger":  0.10,
    "momentum":   0.20,
    "volume":     0.15,
    "earnings":   0.10,
    "sentiment":  0.15,
}

SCORE_THRESHOLDS = {
    "BUY":   0.62,
    "WATCH": 0.42,
    # Below WATCH → AVOID
}

# ═══════════════════════════════════════════════════════════════
#  DATA CLASSES
# ═══════════════════════════════════════════════════════════════

@dataclass
class Signal:
    name: str
    score: float          # 0.0 (bearish) → 1.0 (bullish)
    detail: str           # human-readable explanation
    raw: dict = field(default_factory=dict)

@dataclass
class StockResult:
    ticker: str
    company: str
    price: float
    change_pct: float
    signals: list[Signal] = field(default_factory=list)
    composite_score: float = 0.0
    rating: str = "AVOID"
    news_headlines: list[str] = field(default_factory=list)
    next_earnings: Optional[str] = None
    error: Optional[str] = None


# ═══════════════════════════════════════════════════════════════
#  TECHNICAL ANALYSIS ENGINE
# ═══════════════════════════════════════════════════════════════

def compute_rsi(series: pd.Series, period: int = 14) -> pd.Series:
    delta = series.diff()
    gain = delta.clip(lower=0)
    loss = -delta.clip(upper=0)
    avg_gain = gain.ewm(com=period - 1, min_periods=period).mean()
    avg_loss = loss.ewm(com=period - 1, min_periods=period).mean()
    rs = avg_gain / avg_loss.replace(0, np.nan)
    return 100 - (100 / (1 + rs))


def compute_macd(series: pd.Series, fast=12, slow=26, signal=9):
    ema_fast = series.ewm(span=fast, adjust=False).mean()
    ema_slow = series.ewm(span=slow, adjust=False).mean()
    macd_line = ema_fast - ema_slow
    signal_line = macd_line.ewm(span=signal, adjust=False).mean()
    histogram = macd_line - signal_line
    return macd_line, signal_line, histogram


def compute_bollinger(series: pd.Series, period=20, std_dev=2):
    middle = series.rolling(window=period).mean()
    std = series.rolling(window=period).std()
    upper = middle + std_dev * std
    lower = middle - std_dev * std
    return upper, middle, lower


def analyze_technicals(hist: pd.DataFrame) -> list[Signal]:
    signals = []
    close = hist["Close"].squeeze()
    volume = hist["Volume"].squeeze()

    if len(close) < 30:
        return signals

    # ── RSI ──────────────────────────────────────────────────
    rsi = compute_rsi(close)
    current_rsi = rsi.iloc[-1]
    prev_rsi = rsi.iloc[-3]  # 3 bars ago for trend

    if current_rsi < 30:
        rsi_score = 0.90  # Oversold bounce setup
        rsi_detail = f"RSI {current_rsi:.1f} — oversold, bounce candidate"
    elif current_rsi < 40:
        rsi_score = 0.75
        rsi_detail = f"RSI {current_rsi:.1f} — approaching oversold, dip buy zone"
    elif current_rsi < 55 and current_rsi > prev_rsi:
        rsi_score = 0.65  # Recovering from dip
        rsi_detail = f"RSI {current_rsi:.1f} — recovering upward trend"
    elif current_rsi < 65:
        rsi_score = 0.55  # Neutral-bullish
        rsi_detail = f"RSI {current_rsi:.1f} — neutral, healthy range"
    elif current_rsi < 75:
        rsi_score = 0.40  # Getting extended
        rsi_detail = f"RSI {current_rsi:.1f} — overbought caution"
    else:
        rsi_score = 0.20  # Very overbought
        rsi_detail = f"RSI {current_rsi:.1f} — extremely overbought"

    signals.append(Signal("RSI", rsi_score, rsi_detail,
                          {"rsi": round(current_rsi, 2)}))

    # ── MACD ─────────────────────────────────────────────────
    macd_line, signal_line, histogram = compute_macd(close)
    macd_now = macd_line.iloc[-1]
    macd_prev = macd_line.iloc[-2]
    sig_now = signal_line.iloc[-1]
    sig_prev = signal_line.iloc[-2]
    hist_now = histogram.iloc[-1]
    hist_prev = histogram.iloc[-2]

    bullish_crossover = (macd_prev < sig_prev) and (macd_now > sig_now)
    histogram_expanding = hist_now > hist_prev > 0
    macd_above = macd_now > sig_now

    if bullish_crossover:
        macd_score = 0.90
        macd_detail = f"MACD bullish crossover! Line: {macd_now:.3f}, Signal: {sig_now:.3f}"
    elif macd_above and histogram_expanding:
        macd_score = 0.75
        macd_detail = f"MACD above signal & expanding momentum ({hist_now:.3f})"
    elif macd_above:
        macd_score = 0.60
        macd_detail = f"MACD above signal, momentum slowing ({hist_now:.3f})"
    elif hist_now > hist_prev:  # Narrowing bearish gap = bottoming
        macd_score = 0.45
        macd_detail = f"MACD below signal but histogram improving"
    else:
        macd_score = 0.25
        macd_detail = f"MACD bearish — line {macd_now:.3f} below signal {sig_now:.3f}"

    signals.append(Signal("MACD", macd_score, macd_detail,
                          {"macd": round(macd_now, 4), "signal": round(sig_now, 4),
                           "histogram": round(hist_now, 4)}))

    # ── BOLLINGER BANDS ──────────────────────────────────────
    bb_upper, bb_mid, bb_lower = compute_bollinger(close)
    price_now = close.iloc[-1]
    bb_u = bb_upper.iloc[-1]
    bb_m = bb_mid.iloc[-1]
    bb_l = bb_lower.iloc[-1]
    bb_width = (bb_u - bb_l) / bb_m  # Band width as % of price
    pct_b = (price_now - bb_l) / (bb_u - bb_l) if (bb_u - bb_l) > 0 else 0.5

    if pct_b < 0.05:  # Near or below lower band
        bb_score = 0.88
        bb_detail = f"Price near lower BB (${bb_l:.2f}) — mean reversion setup"
    elif pct_b < 0.25:
        bb_score = 0.72
        bb_detail = f"Price in lower BB zone, %B={pct_b:.2f}"
    elif pct_b < 0.60:
        bb_score = 0.55
        bb_detail = f"Price in mid BB zone, %B={pct_b:.2f}"
    elif pct_b < 0.85:
        bb_score = 0.45
        bb_detail = f"Price in upper BB zone, %B={pct_b:.2f}"
    else:
        bb_score = 0.20
        bb_detail = f"Price at/above upper BB (${bb_u:.2f}) — extended"

    # Squeeze bonus: tight bands often precede breakout
    if bb_width < 0.05:
        bb_score = min(bb_score + 0.10, 1.0)
        bb_detail += " + Bollinger Squeeze detected!"

    signals.append(Signal("Bollinger Bands", bb_score, bb_detail,
                          {"upper": round(bb_u, 2), "mid": round(bb_m, 2),
                           "lower": round(bb_l, 2), "pct_b": round(pct_b, 3),
                           "width": round(bb_width, 4)}))

    # ── MOMENTUM (Price Rate of Change) ──────────────────────
    roc_5 = (close.iloc[-1] / close.iloc[-6] - 1) * 100   # 5-day
    roc_10 = (close.iloc[-1] / close.iloc[-11] - 1) * 100  # 10-day
    roc_20 = (close.iloc[-1] / close.iloc[-21] - 1) * 100  # 20-day

    # SMA trend filter
    sma20 = close.rolling(20).mean().iloc[-1]
    sma50 = close.rolling(50).mean().iloc[-1] if len(close) >= 50 else sma20
    above_sma20 = price_now > sma20
    above_sma50 = price_now > sma50
    sma_trend = (close.rolling(20).mean().diff().iloc[-1]) > 0

    mom_score = 0.50
    if roc_5 > 0 and roc_10 > 0 and above_sma20:
        mom_score = 0.80
        mom_detail = f"Strong uptrend: +{roc_5:.1f}% (5d), +{roc_10:.1f}% (10d), above SMA20"
    elif roc_5 > 2 and not above_sma20:
        mom_score = 0.65
        mom_detail = f"Recovering: +{roc_5:.1f}% (5d) but below SMA20"
    elif roc_5 > 0 and roc_10 < 0:
        mom_score = 0.52
        mom_detail = f"Short-term bounce: +{roc_5:.1f}% (5d) but 10d still negative"
    elif roc_5 < -3:
        mom_score = 0.25
        mom_detail = f"Selling pressure: {roc_5:.1f}% (5d), {roc_10:.1f}% (10d)"
    else:
        mom_detail = f"Neutral momentum: {roc_5:.1f}% (5d), {roc_10:.1f}% (10d)"

    # EMA crossover bonus
    ema9 = close.ewm(span=9, adjust=False).mean()
    ema21 = close.ewm(span=21, adjust=False).mean()
    if ema9.iloc[-1] > ema21.iloc[-1] and ema9.iloc[-2] <= ema21.iloc[-2]:
        mom_score = min(mom_score + 0.12, 1.0)
        mom_detail += " + EMA9/21 bullish crossover!"

    signals.append(Signal("Momentum", mom_score, mom_detail,
                          {"roc_5d": round(roc_5, 2), "roc_10d": round(roc_10, 2),
                           "roc_20d": round(roc_20, 2), "above_sma20": above_sma20,
                           "above_sma50": above_sma50}))

    # ── VOLUME ANALYSIS ───────────────────────────────────────
    avg_vol_20 = volume.rolling(20).mean().iloc[-1]
    today_vol = volume.iloc[-1]
    vol_ratio = today_vol / avg_vol_20 if avg_vol_20 > 0 else 1.0

    # Volume on up days vs down days
    price_changes = close.diff().iloc[-5:]
    vol_recent = volume.iloc[-5:]
    up_vol = vol_recent[price_changes > 0].sum()
    down_vol = vol_recent[price_changes <= 0].sum()
    vol_trend_bullish = up_vol > down_vol * 1.2 if down_vol > 0 else True

    if vol_ratio > 2.0 and close.diff().iloc[-1] > 0:
        vol_score = 0.88
        vol_detail = f"High-volume breakout! {vol_ratio:.1f}x avg volume on up day"
    elif vol_ratio > 1.5 and vol_trend_bullish:
        vol_score = 0.75
        vol_detail = f"Elevated volume ({vol_ratio:.1f}x avg) with bullish bias"
    elif vol_ratio > 1.2:
        vol_score = 0.62
        vol_detail = f"Above-average volume ({vol_ratio:.1f}x), accumulation possible"
    elif vol_ratio < 0.5:
        vol_score = 0.40
        vol_detail = f"Very low volume ({vol_ratio:.1f}x avg) — low conviction"
    elif vol_trend_bullish:
        vol_score = 0.58
        vol_detail = f"Normal volume with bullish up/down distribution"
    else:
        vol_score = 0.42
        vol_detail = f"Normal volume but distribution skews bearish"

    signals.append(Signal("Volume", vol_score, vol_detail,
                          {"vol_ratio": round(vol_ratio, 2), "avg_vol_20d": int(avg_vol_20),
                           "today_vol": int(today_vol)}))

    return signals


# ═══════════════════════════════════════════════════════════════
#  EARNINGS CATALYST DETECTOR
# ═══════════════════════════════════════════════════════════════

def analyze_earnings(info: dict, ticker: str) -> tuple[Signal, Optional[str]]:
    """
    Looks for upcoming earnings within 1–10 days (catalyst window).
    Returns a Signal and the earnings date string.
    """
    earnings_date = None
    earnings_str = None

    # yfinance stores next earnings in calendar
    try:
        cal = info.get("earningsDate", None)
        if cal:
            if isinstance(cal, (list, tuple)) and len(cal) > 0:
                earnings_date = pd.Timestamp(cal[0])
            elif hasattr(cal, '__iter__'):
                earnings_date = pd.Timestamp(list(cal)[0])
    except Exception:
        pass

    now = datetime.now()
    days_to_earnings = None

    if earnings_date:
        earnings_str = earnings_date.strftime("%b %d, %Y")
        days_to_earnings = (earnings_date - pd.Timestamp(now)).days

    if days_to_earnings is not None:
        if 0 <= days_to_earnings <= 3:
            score = 0.80
            detail = f"Earnings in {days_to_earnings}d ({earnings_str}) — high-volatility catalyst!"
        elif 4 <= days_to_earnings <= 10:
            score = 0.72
            detail = f"Earnings in {days_to_earnings}d ({earnings_str}) — in swing window"
        elif -3 <= days_to_earnings < 0:
            score = 0.60
            detail = f"Just reported ({abs(days_to_earnings)}d ago) — post-earnings drift possible"
        else:
            score = 0.50
            detail = f"Earnings {earnings_str} — outside swing window"
    else:
        score = 0.50
        detail = "No upcoming earnings date found"

    return Signal("Earnings Catalyst", score, detail,
                  {"days_to_earnings": days_to_earnings,
                   "earnings_date": earnings_str}), earnings_str


# ═══════════════════════════════════════════════════════════════
#  NEWS SCRAPER
# ═══════════════════════════════════════════════════════════════

def fetch_news_headlines(ticker: str, max_headlines: int = 5) -> list[str]:
    """Scrapes Yahoo Finance news for the ticker."""
    headlines = []
    try:
        url = f"https://finance.yahoo.com/quote/{ticker}/news/"
        headers = {
            "User-Agent": (
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                "AppleWebKit/537.36 (KHTML, like Gecko) "
                "Chrome/120.0.0.0 Safari/537.36"
            )
        }
        resp = requests.get(url, headers=headers, timeout=8)
        soup = BeautifulSoup(resp.text, "html.parser")

        # Try multiple CSS selectors (Yahoo Finance changes layout often)
        for selector in ["h3", "h2", "[data-test='story-title']", ".Mb(5px)"]:
            tags = soup.select(selector)
            for tag in tags:
                text = tag.get_text(strip=True)
                if len(text) > 20 and ticker.upper() not in text[:5]:
                    headlines.append(text)
                if len(headlines) >= max_headlines:
                    break
            if headlines:
                break

    except Exception as e:
        headlines = [f"[News fetch error: {e}]"]

    return headlines[:max_headlines]


# ═══════════════════════════════════════════════════════════════
#  AI NEWS SENTIMENT (Claude)
# ═══════════════════════════════════════════════════════════════

_claude_client = None

def get_claude_client():
    global _claude_client
    if _claude_client is None and CLAUDE_AVAILABLE:
        _claude_client = anthropic.Anthropic(
            api_key=os.environ["ANTHROPIC_API_KEY"]
        )
    return _claude_client


def analyze_sentiment_with_claude(ticker: str, headlines: list[str]) -> Signal:
    """
    Sends headlines to Claude and asks for a structured sentiment score.
    Falls back to simple keyword scoring if Claude is unavailable.
    """
    if not headlines:
        return Signal("News Sentiment", 0.50, "No headlines to analyze")

    client = get_claude_client()

    if client:
        # ── Claude-powered sentiment ──────────────────────────
        prompt = f"""You are a quantitative equity analyst. Analyze these recent news headlines for {ticker} and return a JSON object ONLY (no extra text).

Headlines:
{chr(10).join(f'- {h}' for h in headlines)}

Return this exact JSON structure:
{{
  "score": <float 0.0-1.0 where 0=very bearish, 0.5=neutral, 1.0=very bullish>,
  "sentiment": "<BULLISH|NEUTRAL|BEARISH>",
  "summary": "<one sentence max 20 words explaining the key sentiment driver>",
  "catalysts": ["<catalyst1>", "<catalyst2>"]
}}

Focus on: earnings surprises, revenue growth, partnerships, product launches, regulatory issues, lawsuits, analyst upgrades/downgrades, macro headwinds."""

        try:
            response = client.messages.create(
                model="claude-sonnet-4-5-20250929",
                max_tokens=300,
                messages=[{"role": "user", "content": prompt}]
            )
            raw = response.content[0].text.strip()
            # Strip markdown code fences if present
            raw = raw.replace("```json", "").replace("```", "").strip()
            data = json.loads(raw)

            score = float(data.get("score", 0.5))
            sentiment = data.get("sentiment", "NEUTRAL")
            summary = data.get("summary", "")
            catalysts = data.get("catalysts", [])

            detail = f"[Claude] {sentiment}: {summary}"
            if catalysts:
                detail += f" | Catalysts: {', '.join(catalysts[:2])}"

            return Signal("News Sentiment (AI)", score, detail,
                         {"sentiment": sentiment, "catalysts": catalysts,
                          "ai_powered": True})

        except Exception as e:
            # Fall through to keyword fallback
            pass

    # ── Keyword fallback sentiment ────────────────────────────
    bullish_words = {
        "beat", "beats", "surge", "soars", "upgrade", "upgraded", "buy",
        "outperform", "record", "growth", "gains", "rally", "strong",
        "raised", "raises", "guidance", "exceeds", "partnership", "wins",
        "bullish", "breakout", "positive", "profit", "revenue", "expands"
    }
    bearish_words = {
        "miss", "misses", "plunge", "falls", "downgrade", "downgraded",
        "sell", "underperform", "loss", "losses", "weak", "cut", "cuts",
        "lowers", "concerns", "lawsuit", "investigation", "recall",
        "bearish", "breakdown", "negative", "debt", "layoffs", "fired"
    }

    all_text = " ".join(headlines).lower()
    bull_count = sum(1 for w in bullish_words if w in all_text)
    bear_count = sum(1 for w in bearish_words if w in all_text)

    total = bull_count + bear_count
    if total == 0:
        score = 0.50
        detail = "News sentiment: neutral/mixed"
    else:
        score = bull_count / total
        if score > 0.65:
            detail = f"News sentiment: BULLISH ({bull_count} bullish, {bear_count} bearish signals)"
        elif score < 0.35:
            detail = f"News sentiment: BEARISH ({bull_count} bullish, {bear_count} bearish signals)"
        else:
            detail = f"News sentiment: MIXED ({bull_count} bullish, {bear_count} bearish signals)"

    return Signal("News Sentiment", score, detail,
                  {"bullish_signals": bull_count, "bearish_signals": bear_count,
                   "ai_powered": False})


# ═══════════════════════════════════════════════════════════════
#  COMPOSITE SCORING
# ═══════════════════════════════════════════════════════════════

SIGNAL_NAME_MAP = {
    "RSI": "rsi",
    "MACD": "macd",
    "Bollinger Bands": "bollinger",
    "Momentum": "momentum",
    "Volume": "volume",
    "Earnings Catalyst": "earnings",
    "News Sentiment": "sentiment",
    "News Sentiment (AI)": "sentiment",
}

def compute_composite_score(signals: list[Signal]) -> float:
    total_weight = 0.0
    weighted_sum = 0.0
    for sig in signals:
        key = SIGNAL_NAME_MAP.get(sig.name, "")
        weight = WEIGHTS.get(key, 0.0)
        weighted_sum += sig.score * weight
        total_weight += weight
    return weighted_sum / total_weight if total_weight > 0 else 0.0


def assign_rating(score: float) -> str:
    if score >= SCORE_THRESHOLDS["BUY"]:
        return "🟢 BUY"
    elif score >= SCORE_THRESHOLDS["WATCH"]:
        return "🟡 WATCH"
    else:
        return "🔴 AVOID"


# ═══════════════════════════════════════════════════════════════
#  MAIN SCANNER
# ═══════════════════════════════════════════════════════════════

def scan_ticker(ticker: str, fetch_news: bool = True) -> StockResult:
    ticker = ticker.upper().strip()
    result = StockResult(ticker=ticker, company=ticker, price=0.0, change_pct=0.0)

    try:
        stock = yf.Ticker(ticker)

        # ── Price history ─────────────────────────────────────
        hist = stock.history(period="3mo", interval="1d")
        if hist.empty or len(hist) < 30:
            result.error = "Insufficient price history"
            return result

        result.price = round(float(hist["Close"].iloc[-1]), 2)
        result.change_pct = round(
            float((hist["Close"].iloc[-1] / hist["Close"].iloc[-2] - 1) * 100), 2
        )

        # ── Company info ──────────────────────────────────────
        info = stock.info or {}
        result.company = info.get("longName", info.get("shortName", ticker))

        # ── Technical signals ─────────────────────────────────
        tech_signals = analyze_technicals(hist)
        result.signals.extend(tech_signals)

        # ── Earnings catalyst ─────────────────────────────────
        earnings_signal, earnings_date = analyze_earnings(info, ticker)
        result.signals.append(earnings_signal)
        result.next_earnings = earnings_date

        # ── News + sentiment ──────────────────────────────────
        if fetch_news:
            headlines = fetch_news_headlines(ticker)
            result.news_headlines = headlines
            sentiment_signal = analyze_sentiment_with_claude(ticker, headlines)
            result.signals.append(sentiment_signal)
        else:
            result.signals.append(
                Signal("News Sentiment", 0.50, "News fetch skipped")
            )

        # ── Final score & rating ──────────────────────────────
        result.composite_score = round(compute_composite_score(result.signals), 3)
        result.rating = assign_rating(result.composite_score)

    except Exception as e:
        result.error = str(e)

    return result


def scan_all(tickers: list[str], verbose: bool = True) -> list[StockResult]:
    results = []
    total = len(tickers)

    for i, ticker in enumerate(tickers, 1):
        if verbose:
            print(f"  [{i:>3}/{total}] Scanning {ticker:<8}", end="", flush=True)

        result = scan_ticker(ticker)

        if verbose:
            if result.error:
                print(f"  ❌ {result.error}")
            else:
                bar = "█" * int(result.composite_score * 20)
                print(
                    f"  {result.rating:<14}  "
                    f"Score: {result.composite_score:.3f}  "
                    f"|{bar:<20}|"
                )

        results.append(result)
        time.sleep(0.3)  # polite throttle

    return results


# ═══════════════════════════════════════════════════════════════
#  S&P 500 TICKER LIST
# ═══════════════════════════════════════════════════════════════

def get_sp500_tickers() -> list[str]:
    """Scrapes Wikipedia for S&P 500 constituents."""
    try:
        url = "https://en.wikipedia.org/wiki/List_of_S%26P_500_companies"
        tables = pd.read_html(url)
        df = tables[0]
        tickers = df["Symbol"].str.replace(".", "-", regex=False).tolist()
        print(f"✓ Loaded {len(tickers)} S&P 500 tickers from Wikipedia")
        return tickers
    except Exception as e:
        print(f"⚠ Could not fetch S&P 500 list ({e}), using default watchlist")
        return DEFAULT_WATCHLIST


# ═══════════════════════════════════════════════════════════════
#  CONSOLE REPORT
# ═══════════════════════════════════════════════════════════════

def print_report(results: list[StockResult]):
    print("\n")
    print("═" * 80)
    print("  SWING TRADE SCANNER RESULTS  —  " + datetime.now().strftime("%Y-%m-%d %H:%M"))
    print("  Holding Period: 1–10 Days | Data: Yahoo Finance")
    print("═" * 80)

    # Sort by composite score descending
    valid = [r for r in results if not r.error]
    errored = [r for r in results if r.error]
    valid.sort(key=lambda x: x.composite_score, reverse=True)

    buys  = [r for r in valid if "BUY" in r.rating]
    watch = [r for r in valid if "WATCH" in r.rating]
    avoid = [r for r in valid if "AVOID" in r.rating]

    for category, stocks in [("TOP PICKS — BUY", buys),
                              ("WATCHLIST", watch),
                              ("AVOID / HOLD", avoid)]:
        if not stocks:
            continue
        print(f"\n{'─'*80}")
        print(f"  {category}")
        print(f"{'─'*80}")
        for r in stocks:
            change_str = f"+{r.change_pct:.2f}%" if r.change_pct >= 0 else f"{r.change_pct:.2f}%"
            print(f"\n  {r.ticker:<6}  {r.company[:35]:<35}  "
                  f"${r.price:<8.2f}  {change_str:<8}  Score: {r.composite_score:.3f}")
            for sig in r.signals:
                icon = "▲" if sig.score >= 0.65 else "▼" if sig.score < 0.40 else "─"
                print(f"    {icon} {sig.name:<24} {sig.score:.2f}  {sig.detail}")
            if r.news_headlines:
                print(f"    📰 Headlines: {r.news_headlines[0][:70]}")
            if r.next_earnings:
                print(f"    📅 Next Earnings: {r.next_earnings}")

    if errored:
        print(f"\n{'─'*80}")
        print(f"  ERRORS ({len(errored)} tickers)")
        for r in errored:
            print(f"  ✗ {r.ticker}: {r.error}")

    print(f"\n{'═'*80}")
    print(f"  SUMMARY: {len(buys)} BUY  |  {len(watch)} WATCH  |  {len(avoid)} AVOID")
    print(f"  AI Sentiment: {'✓ Claude-powered' if CLAUDE_AVAILABLE else '✗ Keyword fallback (set ANTHROPIC_API_KEY to enable Claude)'}")
    print(f"{'═'*80}\n")


# ═══════════════════════════════════════════════════════════════
#  HTML REPORT GENERATOR
# ═══════════════════════════════════════════════════════════════

def save_html_report(results: list[StockResult], filename: str = "swing_scan_report.html"):
    valid = sorted([r for r in results if not r.error],
                   key=lambda x: x.composite_score, reverse=True)

    def score_color(score):
        if score >= 0.62: return "#22c55e"
        if score >= 0.42: return "#f59e0b"
        return "#ef4444"

    def score_bar(score):
        pct = int(score * 100)
        color = score_color(score)
        return (f'<div style="background:#1e293b;border-radius:4px;height:8px;width:100%">'
                f'<div style="background:{color};border-radius:4px;height:8px;width:{pct}%"></div></div>')

    rows = ""
    for r in valid:
        signals_html = ""
        for sig in r.signals:
            icon = "▲" if sig.score >= 0.65 else "▼" if sig.score < 0.40 else "─"
            color = score_color(sig.score)
            signals_html += (
                f'<div style="font-size:11px;padding:2px 0;color:#94a3b8">'
                f'<span style="color:{color};font-weight:bold">{icon} {sig.name}</span>'
                f' <span style="color:#64748b">{sig.score:.2f}</span>'
                f' — {sig.detail}</div>'
            )

        news_html = ""
        for h in r.news_headlines[:3]:
            news_html += f'<div style="font-size:10px;color:#64748b;padding:1px 0">📰 {h[:80]}</div>'

        change_color = "#22c55e" if r.change_pct >= 0 else "#ef4444"
        change_str = f"+{r.change_pct:.2f}%" if r.change_pct >= 0 else f"{r.change_pct:.2f}%"

        rating_color = score_color(r.composite_score)

        rows += f"""
        <tr>
            <td style="padding:16px 12px;border-bottom:1px solid #1e293b;vertical-align:top">
                <div style="font-size:18px;font-weight:800;color:#f1f5f9;font-family:'Space Mono',monospace">{r.ticker}</div>
                <div style="font-size:11px;color:#64748b;max-width:180px">{r.company[:35]}</div>
            </td>
            <td style="padding:16px 12px;border-bottom:1px solid #1e293b;vertical-align:top;white-space:nowrap">
                <div style="font-size:20px;font-weight:700;color:#f1f5f9">${r.price:.2f}</div>
                <div style="font-size:13px;color:{change_color}">{change_str}</div>
            </td>
            <td style="padding:16px 12px;border-bottom:1px solid #1e293b;vertical-align:top;min-width:140px">
                <div style="font-size:22px;font-weight:800;color:{rating_color}">{r.composite_score:.3f}</div>
                <div style="margin-top:4px">{score_bar(r.composite_score)}</div>
                <div style="margin-top:6px;font-size:12px;font-weight:700;color:{rating_color}">{r.rating}</div>
            </td>
            <td style="padding:16px 12px;border-bottom:1px solid #1e293b;vertical-align:top;min-width:340px">
                {signals_html}
            </td>
            <td style="padding:16px 12px;border-bottom:1px solid #1e293b;vertical-align:top;max-width:240px">
                {news_html}
                {"<div style='font-size:10px;color:#818cf8;margin-top:4px'>📅 " + r.next_earnings + "</div>" if r.next_earnings else ""}
            </td>
        </tr>"""

    html = f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Swing Trade Scanner — {datetime.now().strftime('%Y-%m-%d')}</title>
<link href="https://fonts.googleapis.com/css2?family=Space+Mono:wght@400;700&family=Inter:wght@300;400;600;700;800&display=swap" rel="stylesheet">
<style>
  * {{ margin:0; padding:0; box-sizing:border-box; }}
  body {{ background:#0a0f1e; color:#f1f5f9; font-family:'Inter',sans-serif; padding:32px; }}
  h1 {{ font-family:'Space Mono',monospace; font-size:28px; color:#f1f5f9; letter-spacing:-1px; }}
  .subtitle {{ color:#475569; font-size:13px; margin-top:6px; margin-bottom:32px; }}
  .badge {{ display:inline-block; padding:3px 10px; border-radius:20px; font-size:11px; font-weight:700; margin-right:8px; }}
  .buy   {{ background:#064e3b; color:#34d399; }}
  .watch {{ background:#451a03; color:#fbbf24; }}
  .avoid {{ background:#450a0a; color:#f87171; }}
  table {{ width:100%; border-collapse:collapse; }}
  thead {{ background:#0f172a; }}
  th {{ padding:12px; text-align:left; font-size:11px; font-weight:700; color:#475569; text-transform:uppercase; letter-spacing:.08em; border-bottom:2px solid #1e293b; }}
  tr:hover td {{ background:rgba(255,255,255,0.02); }}
  .footer {{ margin-top:32px; font-size:11px; color:#334155; text-align:center; }}
</style>
</head>
<body>
<h1>⚡ Swing Trade Scanner</h1>
<div class="subtitle">
  Generated {datetime.now().strftime('%B %d, %Y at %H:%M')} &nbsp;·&nbsp;
  Holding period: 1–10 days &nbsp;·&nbsp;
  Data: Yahoo Finance &nbsp;·&nbsp;
  AI Sentiment: {'✓ Claude' if CLAUDE_AVAILABLE else '✗ Keyword fallback'}
  &nbsp;&nbsp;
  <span class="badge buy">{len([r for r in valid if 'BUY' in r.rating])} BUY</span>
  <span class="badge watch">{len([r for r in valid if 'WATCH' in r.rating])} WATCH</span>
  <span class="badge avoid">{len([r for r in valid if 'AVOID' in r.rating])} AVOID</span>
</div>
<table>
  <thead>
    <tr>
      <th>Ticker</th>
      <th>Price</th>
      <th>Score / Rating</th>
      <th>Signal Breakdown</th>
      <th>News & Catalyst</th>
    </tr>
  </thead>
  <tbody>
    {rows}
  </tbody>
</table>
<div class="footer">
  ⚠️ This tool is for informational purposes only. Not financial advice. Always do your own research.
</div>
</body>
</html>"""

    with open(filename, "w") as f:
        f.write(html)
    print(f"\n  📊 HTML report saved → {filename}")


# ═══════════════════════════════════════════════════════════════
#  CLI ENTRY POINT
# ═══════════════════════════════════════════════════════════════

def main():
    parser = argparse.ArgumentParser(
        description="Low-frequency swing trade scanner (1–10 day holds)",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__
    )
    parser.add_argument(
        "--tickers", nargs="+", metavar="TICKER",
        help="Specific tickers to scan (e.g. --tickers AAPL MSFT NVDA)"
    )
    parser.add_argument(
        "--sp500", action="store_true",
        help="Scan all S&P 500 stocks (slow — ~500 requests)"
    )
    parser.add_argument(
        "--report", action="store_true",
        help="Save results as HTML report"
    )
    parser.add_argument(
        "--report-file", default="swing_scan_report.html",
        help="HTML report filename (default: swing_scan_report.html)"
    )
    parser.add_argument(
        "--top", type=int, default=0,
        help="Only show top N results by score"
    )
    parser.add_argument(
        "--quiet", action="store_true",
        help="Suppress per-ticker progress output"
    )
    args = parser.parse_args()

    # ── Choose ticker list ────────────────────────────────────
    if args.tickers:
        tickers = [t.upper() for t in args.tickers]
    elif args.sp500:
        tickers = get_sp500_tickers()
    else:
        tickers = DEFAULT_WATCHLIST

    print(f"\n  SWING TRADE SCANNER  —  {datetime.now().strftime('%Y-%m-%d %H:%M')}")
    print(f"  Scanning {len(tickers)} ticker(s)...\n")

    if CLAUDE_AVAILABLE:
        print("  ✓ Claude AI sentiment enabled\n")
    else:
        print("  ℹ  Keyword sentiment active (set ANTHROPIC_API_KEY for Claude AI)\n")

    # ── Run scan ──────────────────────────────────────────────
    results = scan_all(tickers, verbose=not args.quiet)

    # Filter to top N if requested
    if args.top > 0:
        valid = sorted([r for r in results if not r.error],
                       key=lambda x: x.composite_score, reverse=True)
        errors = [r for r in results if r.error]
        results = valid[:args.top] + errors

    # ── Print console report ──────────────────────────────────
    print_report(results)

    # ── Save HTML report ──────────────────────────────────────
    if args.report:
        save_html_report(results, args.report_file)


if __name__ == "__main__":
    main()
