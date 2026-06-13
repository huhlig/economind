// Typed API client for Economind REST endpoints

const BASE = '';

function authHeaders(): HeadersInit {
  const key = localStorage.getItem('api_key') ?? '';
  return key ? { Authorization: `Bearer ${key}` } : {};
}

async function get<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, { headers: authHeaders() });
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return res.json() as Promise<T>;
}

async function post<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return res.json() as Promise<T>;
}

async function put<T>(path: string, body: unknown): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json', ...authHeaders() },
    body: JSON.stringify(body),
  });
  if (!res.ok) throw new ApiError(res.status, await res.text());
  return res.json() as Promise<T>;
}

export class ApiError extends Error {
  constructor(public status: number, message: string) {
    super(message);
  }
}

// ── Types ──────────────────────────────────────────────────────────────────────

export interface Instrument {
  symbol: string;
  name: string;
  exchange?: string;
  asset_class: string;
  currency: string;
  marketcap?: number;
  sector?: string;
  industry?: string;
}

export interface Signal {
  id: string;
  strategy_config_id: string;
  symbol: string;
  signal_type: string;
  direction: 'Long' | 'Short' | 'Flat';
  strength?: number;
  generated_at: string;
  metadata: Record<string, unknown>;
}

export interface Position {
  symbol: string;
  quantity: number;
  average_cost: number;
  current_price?: number;
  unrealized_pnl?: number;
  side: 'Long' | 'Short';
}

export interface PortfolioSummary {
  positions: Position[];
  total_equity: number;
  cash: number;
  unrealized_pnl: number;
}

export interface StrategyConfig {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  universe: string[];
  plugins: PluginSpec[];
  parameters: Record<string, string>;
  created_at: string;
  updated_at: string;
}

export interface PluginSpec {
  role: string;
  name: string;
}

export interface StrategyRunResult {
  run_id: string;
  strategy_config_id: string;
  status: string;
  signals_generated: number;
  started_at: string;
  completed_at?: string;
  error?: string;
}

export interface BacktestRequest {
  strategy_config_id: string;
  from_date: string;
  to_date: string;
  initial_capital: number;
  commission_rate?: number;
  slippage_bps?: number;
}

export interface BacktestRun {
  id: string;
  strategy_config_id: string;
  from_date: string;
  to_date: string;
  initial_capital: number;
  status: string;
  total_return?: number;
  annualized_return?: number;
  sharpe_ratio?: number;
  max_drawdown?: number;
  total_trades?: number;
  win_rate?: number;
  created_at: string;
  completed_at?: string;
}

export interface BacktestTrade {
  id: string;
  symbol: string;
  side: string;
  entry_price: number;
  exit_price?: number;
  quantity: number;
  net_pnl?: number;
  entry_time: string;
  exit_time?: string;
}

export interface BacktestDetail {
  run: BacktestRun;
  trades: BacktestTrade[];
  equity_curve: EquityPoint[];
}

export interface EquityPoint {
  date: string;
  equity: number;
}

export interface Bar {
  date: string;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

// ── Instruments ────────────────────────────────────────────────────────────────

export const instruments = {
  list: () => get<Instrument[]>('/api/instruments'),
  get: (symbol: string) => get<Instrument>(`/api/instruments/${symbol}`),
};

// ── Signals ────────────────────────────────────────────────────────────────────

export const signals = {
  list: (params?: { strategy_id?: string; symbol?: string; limit?: number }) => {
    const qs = new URLSearchParams();
    if (params?.strategy_id) qs.set('strategy_id', params.strategy_id);
    if (params?.symbol) qs.set('symbol', params.symbol);
    if (params?.limit) qs.set('limit', String(params.limit));
    const q = qs.toString();
    return get<Signal[]>(`/api/signals${q ? '?' + q : ''}`);
  },
  get: (id: string) => get<Signal>(`/api/signals/${id}`),
};

// ── Portfolio ──────────────────────────────────────────────────────────────────

export const portfolio = {
  summary: () => get<PortfolioSummary>('/api/positions'),
};

// ── Strategy ───────────────────────────────────────────────────────────────────

export const strategy = {
  list: () => get<StrategyConfig[]>('/api/strategy/configs'),
  get: (id: string) => get<StrategyConfig>(`/api/strategy/configs/${id}`),
  update: (id: string, body: Partial<StrategyConfig>) =>
    put<StrategyConfig>(`/api/strategy/configs/${id}`, body),
  run: (id: string) => post<StrategyRunResult>(`/api/strategy/configs/${id}/run`, {}),
  runs: (id: string) => get<StrategyRunResult[]>(`/api/strategy/configs/${id}/runs`),
};

// ── Backtest ───────────────────────────────────────────────────────────────────

export const backtest = {
  run: (req: BacktestRequest) => post<BacktestRun>('/api/backtest/run', req),
  list: () => get<BacktestRun[]>('/api/backtest/runs'),
  get: (id: string) => get<BacktestDetail>(`/api/backtest/runs/${id}`),
};

// ── Data ────────────────────────────────────────────────────────────────────────

export const data = {
  bars: (symbol: string, from: string, to: string) =>
    get<Bar[]>(`/api/data/bars/${symbol}?from=${from}&to=${to}`),
};
