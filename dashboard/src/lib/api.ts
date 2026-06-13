// Typed API client for Economind REST endpoints

const BASE = '/api/v1';

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

async function del<T>(path: string): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    method: 'DELETE',
    headers: authHeaders(),
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
  run_id?: string;
  config_id?: string;
  strategy_config_id: string;
  symbol: string;
  signal_type: string;
  direction: 'Long' | 'Short' | 'Flat';
  strength?: number;
  generated_at: string;
  emitted_at?: string;
  metadata: Record<string, unknown>;
}

export interface Position {
  id: string;
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

export interface OpenPositionRecord {
  id: string;
  symbol: string;
  shares: string;
  entry_price: string;
  entry_at: string;
}

export interface BuyRequest {
  symbol: string;
  shares: string;
  entry_price: string;
  entry_at?: string;
}

export interface SellRequest {
  exit_price: string;
  exit_at?: string;
}

export interface WatchItem {
  symbol: string;
  added_at: string;
}

export interface StrategyConfig {
  id: string;
  name: string;
  description?: string;
  enabled: boolean;
  universe?: string[];
  composition?: string;
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
  config_id: string;
  status: string;
  signals_generated?: number;
  started_at: string;
  completed_at?: string;
  error?: string;
}

export interface BacktestRequest {
  config_id: string;
  from_date: string;
  to_date: string;
  initial_capital: number;
  commission_per_trade?: number;
  slippage_bps?: number;
}

export interface BacktestRun {
  id: string;
  config_id: string;
  from_date: string;
  to_date: string;
  initial_capital: number;
  final_capital?: number;
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

export interface IngestResult {
  status: string;
  summary: string;
}

export interface DatafeedFetchResult {
  status: string;
  provider: string;
  action: string;
  ticker?: string;
  message: string;
}

export interface ChatMessage {
  role: 'user' | 'assistant' | string;
  content: string;
}

export interface ChatPersona {
  id: string;
  name?: string;
  description: string;
  visible?: boolean;
}

export interface ChatResponse {
  message: string;
  history: ChatMessage[];
  session: ChatSession;
  persona_id?: string;
}

export interface ChatSession {
  id: string;
  title: string;
  persona_id?: string;
  depth?: 'basic' | 'detailed' | 'expert';
  created_at: string;
  updated_at: string;
}

export interface ChatSessionDetail {
  session: ChatSession;
  messages: Array<ChatMessage & {
    id: string;
    session_id: string;
    ordinal: number;
    created_at: string;
  }>;
  history: ChatMessage[];
}

export interface LlmSettings {
  provider: 'auto' | 'anthropic' | 'local';
  anthropic_model: string;
  local_base_url: string;
  local_model: string;
  anthropic_api_key_configured: boolean;
  source: string;
}

export interface LlmModels {
  provider: string;
  models: string[] | Array<{ id: string; provider: string }>;
}

export interface DatafeedSettings {
  bar_concurrency: number;
  bar_backfill_days: number;
  fred_series: string[];
  alpaca_key_configured: boolean;
  tiingo_key_configured: boolean;
  simfin_key_configured: boolean;
  fred_key_configured: boolean;
}

export interface ScheduleSettings {
  enabled: boolean;
  bars_utc: string;
  macro_utc: string;
  fundamentals_utc: string;
  strategy_utc: string;
  bars_lookback_days: number;
}

export interface RiskSettings {
  max_drawdown_pct: number;
  max_position_pct: number;
  max_open_positions: number;
}

export interface NotificationsSettings {
  webhook_url: string | null;
  on_signal: boolean;
  on_run_complete: boolean;
  on_order: boolean;
  on_error: boolean;
}

interface BarsResponse {
  bars: Bar[];
}

interface BacktestListItem {
  run_id: string;
  config_id: string;
  started_at: string;
  completed_at?: string;
  status: string;
  total_trades: number;
  cagr?: string;
  sharpe_ratio?: string;
}

interface BacktestSummary {
  run_id: string;
  config_id: string;
  from_date: string;
  to_date: string;
  initial_capital: string;
  final_capital: string;
  cagr: string;
  sharpe_ratio: string;
  sortino_ratio: string;
  max_drawdown: string;
  total_trades: number;
  win_rate: string;
  run_at: string;
}

interface BacktestDetailResponse {
  run_id: string;
  config_id: string;
  started_at: string;
  completed_at?: string;
  status: string;
  initial_capital?: string;
  final_capital?: string;
  cagr?: string;
  sharpe_ratio?: string;
  max_drawdown?: string;
  total_trades?: number;
  win_rate?: string;
  trades: Array<{
    id: string;
    symbol: string;
    direction: string;
    entry_price: string;
    exit_price?: string;
    shares: string;
    realized_pnl?: string;
    entry_date: string;
    exit_date?: string;
  }>;
  equity_curve: Array<{ date: string; value: string }>;
}

// ── Instruments ────────────────────────────────────────────────────────────────

export const instruments = {
  list: () => get<Instrument[]>('/instruments'),
  get: (symbol: string) => get<Instrument>(`/instruments/${symbol}`),
};

// ── Signals ────────────────────────────────────────────────────────────────────

export const signals = {
  list: (params?: { strategy_id?: string; symbol?: string; limit?: number }) => {
    const qs = new URLSearchParams();
    if (params?.strategy_id) qs.set('strategy', params.strategy_id);
    if (params?.symbol) qs.set('symbol', params.symbol);
    if (params?.limit) qs.set('limit', String(params.limit));
    const q = qs.toString();
    return get<Signal[]>(`/signals${q ? '?' + q : ''}`).then(rows => rows.map(toSignal));
  },
  get: (id: string) => get<Signal>(`/signals/${id}`).then(toSignal),
};

// ── Portfolio ──────────────────────────────────────────────────────────────────

interface BackendPortfolioSummary {
  portfolio_value: string;
  available_cash: string;
  current_drawdown: string;
  open_positions: Array<{
    id: string;
    symbol: string;
    shares: string;
    entry_price: string;
    entry_at: string;
  }>;
}

function toPortfolioSummary(raw: BackendPortfolioSummary): PortfolioSummary {
  const equity = toNumber(raw.portfolio_value) ?? 0;
  const cash = toNumber(raw.available_cash) ?? 0;
  const invested = equity - cash;
  return {
    total_equity: equity,
    cash,
    unrealized_pnl: 0,
    positions: raw.open_positions.map(p => ({
      id: p.id,
      symbol: p.symbol,
      quantity: toNumber(p.shares) ?? 0,
      average_cost: toNumber(p.entry_price) ?? 0,
      side: 'Long' as const,
      current_price: undefined,
      unrealized_pnl: undefined,
    })),
  };
}

export const portfolio = {
  summary: () => get<BackendPortfolioSummary>('/positions').then(toPortfolioSummary),
  buy: (req: BuyRequest) => post<OpenPositionRecord>('/positions/buy', req),
  sell: (id: string, req: SellRequest) => post<{ status: string; id: string }>(`/positions/${id}/sell`, req),
  listPositions: () => get<BackendPortfolioSummary>('/positions').then(r => r.open_positions),
  // Watchlist
  listWatches: () => get<WatchItem[]>('/watchlist'),
  addWatch: (symbol: string) => post<WatchItem>('/watchlist', { symbol }),
  removeWatch: (symbol: string) => del<{ status: string; symbol: string }>(`/watchlist/${encodeURIComponent(symbol)}`),
};

// ── Strategy ───────────────────────────────────────────────────────────────────

export interface CreateStrategyRequest {
  name: string;
  description?: string;
  composition?: string;
  enabled?: boolean;
}

export const strategy = {
  list: () => get<StrategyConfig[]>('/strategy/configs'),
  get: (id: string) => get<StrategyConfig>(`/strategy/configs/${id}`),
  create: (req: CreateStrategyRequest) => post<StrategyConfig>('/strategy/configs', req),
  update: (id: string, body: Partial<StrategyConfig>) =>
    put<StrategyConfig>(`/strategy/configs/${id}`, body),
  run: (id: string) => post<StrategyRunResult>('/strategy/run', { config_id: id }),
  runs: (_id: string) => Promise.resolve([] as StrategyRunResult[]),
};

// ── Backtest ───────────────────────────────────────────────────────────────────

export const backtest = {
  run: async (req: BacktestRequest) => toBacktestRun(await post<BacktestSummary>('/backtest/run', req)),
  list: async () => (await get<BacktestListItem[]>('/backtest')).map(toBacktestListRun),
  get: async (id: string) => toBacktestDetail(await get<BacktestDetailResponse>(`/backtest/${id}`)),
};

// ── Data ────────────────────────────────────────────────────────────────────────

export const data = {
  bars: (symbol: string, from: string, to: string) =>
    get<BarsResponse>(`/data/bars?symbol=${encodeURIComponent(symbol)}&from=${from}&to=${to}`).then(r => r.bars),
  ingestBars: (req: { since?: string; concurrency?: number }) =>
    post<IngestResult>('/data/ingest/bars', req),
  ingestMacro: (req: { since?: string; series?: string[] }) =>
    post<IngestResult>('/data/ingest/macro', req),
  ingestFundamentals: (req: { edgar_only?: boolean; simfin_only?: boolean }) =>
    post<IngestResult>('/data/ingest/fundamentals', req),
  fetchRReichel: () => post<DatafeedFetchResult>('/datafeed/rreichel', {}),
  fetchTiingoMetadata: (ticker: string) =>
    post<DatafeedFetchResult>(`/datafeed/tiingo/${encodeURIComponent(ticker)}/metadata`, {}),
  fetchTiingoPrices: (ticker: string) =>
    post<DatafeedFetchResult>(`/datafeed/tiingo/${encodeURIComponent(ticker)}/prices`, {}),
};

// ── Agent Chat ────────────────────────────────────────────────────────────────

export const chat = {
  send: (req: { message: string; history: ChatMessage[]; session_id?: string; persona_id?: string; depth?: 'basic' | 'detailed' | 'expert' }) =>
    post<ChatResponse>('/chat', req),
  personas: () =>
    get<{ personas: ChatPersona[] }>('/chat/personas').then((r) => r.personas),
  sessions: () =>
    get<{ sessions: ChatSession[] }>('/chat/sessions').then((r) => r.sessions),
  session: (id: string) =>
    get<ChatSessionDetail>(`/chat/sessions/${id}`),
};

// ── Settings ─────────────────────────────────────────────────────────────────

export const settings = {
  llm: () => get<LlmSettings>('/settings/llm'),
  updateLlm: (req: Pick<LlmSettings, 'provider' | 'anthropic_model' | 'local_base_url' | 'local_model'>) =>
    put<LlmSettings>('/settings/llm', req),
  llmModels: () => get<LlmModels>('/settings/llm/models'),
  testLlm: () => get<{ ok: boolean; message?: string; error?: string }>('/settings/llm/test'),
  datafeed: () => get<DatafeedSettings>('/settings/datafeed'),
  updateDatafeed: (req: Pick<DatafeedSettings, 'bar_concurrency' | 'bar_backfill_days' | 'fred_series'>) =>
    put<DatafeedSettings>('/settings/datafeed', req),
  schedule: () => get<ScheduleSettings>('/settings/schedule'),
  updateSchedule: (req: ScheduleSettings) =>
    put<ScheduleSettings>('/settings/schedule', req),
  risk: () => get<RiskSettings>('/settings/risk'),
  updateRisk: (req: RiskSettings) =>
    put<RiskSettings>('/settings/risk', req),
  notifications: () => get<NotificationsSettings>('/settings/notifications'),
  updateNotifications: (req: NotificationsSettings) =>
    put<NotificationsSettings>('/settings/notifications', req),
};

function toNumber(value: string | number | undefined): number | undefined {
  if (value == null) return undefined;
  const n = typeof value === 'number' ? value : Number(value);
  return Number.isFinite(n) ? n : undefined;
}

function toSignal(raw: Signal): Signal {
  return {
    ...raw,
    strategy_config_id: raw.strategy_config_id ?? raw.config_id ?? '',
    signal_type: raw.signal_type ?? 'strategy',
    generated_at: raw.generated_at ?? raw.emitted_at ?? '',
    metadata: raw.metadata ?? {},
  };
}

function toBacktestRun(raw: BacktestSummary): BacktestRun {
  const initial = toNumber(raw.initial_capital) ?? 0;
  const final = toNumber(raw.final_capital);
  return {
    id: raw.run_id,
    config_id: raw.config_id,
    from_date: raw.from_date,
    to_date: raw.to_date,
    initial_capital: initial,
    final_capital: final,
    status: 'completed',
    total_return: final == null || initial === 0 ? undefined : (final - initial) / initial,
    annualized_return: toNumber(raw.cagr),
    sharpe_ratio: toNumber(raw.sharpe_ratio),
    max_drawdown: toNumber(raw.max_drawdown),
    total_trades: raw.total_trades,
    win_rate: toNumber(raw.win_rate),
    created_at: raw.run_at,
    completed_at: raw.run_at,
  };
}

function toBacktestListRun(raw: BacktestListItem): BacktestRun {
  return {
    id: raw.run_id,
    config_id: raw.config_id,
    from_date: '',
    to_date: '',
    initial_capital: 0,
    status: raw.status,
    sharpe_ratio: toNumber(raw.sharpe_ratio),
    total_trades: raw.total_trades,
    created_at: raw.started_at,
    completed_at: raw.completed_at,
  };
}

function toBacktestDetail(raw: BacktestDetailResponse): BacktestDetail {
  const initial = toNumber(raw.initial_capital) ?? 0;
  const final = toNumber(raw.final_capital);
  return {
    run: {
      id: raw.run_id,
      config_id: raw.config_id,
      from_date: '',
      to_date: '',
      initial_capital: initial,
      final_capital: final,
      status: raw.status,
      total_return: final == null || initial === 0 ? undefined : (final - initial) / initial,
      annualized_return: toNumber(raw.cagr),
      sharpe_ratio: toNumber(raw.sharpe_ratio),
      max_drawdown: toNumber(raw.max_drawdown),
      total_trades: raw.total_trades,
      win_rate: toNumber(raw.win_rate),
      created_at: raw.started_at,
      completed_at: raw.completed_at,
    },
    trades: raw.trades.map(t => ({
      id: t.id,
      symbol: t.symbol,
      side: t.direction,
      entry_price: toNumber(t.entry_price) ?? 0,
      exit_price: toNumber(t.exit_price),
      quantity: toNumber(t.shares) ?? 0,
      net_pnl: toNumber(t.realized_pnl),
      entry_time: t.entry_date,
      exit_time: t.exit_date,
    })),
    equity_curve: raw.equity_curve.map(p => ({
      date: p.date,
      equity: toNumber(p.value) ?? 0,
    })),
  };
}
