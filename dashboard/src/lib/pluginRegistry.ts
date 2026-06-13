// Plugin registry — describes available plugins and their parameters.
// Keep in sync with strategies/ crate implementations.

export interface ParamDef {
  key: string;
  type: 'number' | 'boolean' | 'string';
  default: string;
  description: string;
}

export interface PluginDef {
  name: string;
  role: 'identifier' | 'timer' | 'sizer';
  description: string;
  params: ParamDef[];
}

export const PLUGIN_ROLES = ['identifier', 'timer', 'sizer'] as const;
export type PluginRole = typeof PLUGIN_ROLES[number];

export const PLUGIN_REGISTRY: PluginDef[] = [
  {
    name: 'momentum',
    role: 'identifier',
    description: 'Ranks symbols by risk-adjusted momentum (rolling return ÷ volatility). Emits top-N candidates.',
    params: [
      { key: 'lookback_days', type: 'number', default: '90', description: 'Rolling return/volatility window in trading days' },
      { key: 'top_n',         type: 'number', default: '20', description: 'Maximum number of candidates to emit' },
      { key: 'min_bars',      type: 'number', default: '30', description: 'Minimum bar count to consider a symbol' },
    ],
  },
  {
    name: 'mean-reversion',
    role: 'timer',
    description: 'Times entries using Bollinger Bands, Z-score, and RSI. Higher score = better oversold dip.',
    params: [
      { key: 'bb_period',         type: 'number', default: '20',  description: 'Bollinger Band SMA period' },
      { key: 'bb_std_devs',       type: 'number', default: '2.0', description: 'Bollinger Band standard deviation multiplier' },
      { key: 'rsi_period',        type: 'number', default: '14',  description: 'RSI period' },
      { key: 'zscore_window',     type: 'number', default: '20',  description: 'Z-score rolling window' },
      { key: 'signal_threshold',  type: 'number', default: '0.4', description: 'Minimum composite score to emit a signal (0–1)' },
    ],
  },
  {
    name: 'trend-follow',
    role: 'timer',
    description: 'Times entries using EMA crossover and ADX trend strength. Emits Long on uptrend, Short on downtrend.',
    params: [
      { key: 'fast_ema',      type: 'number',  default: '12',   description: 'Fast EMA period' },
      { key: 'slow_ema',      type: 'number',  default: '26',   description: 'Slow EMA period' },
      { key: 'adx_period',    type: 'number',  default: '14',   description: 'ADX period' },
      { key: 'adx_threshold', type: 'number',  default: '25.0', description: 'Minimum ADX to confirm a trending environment' },
      { key: 'long_only',     type: 'boolean', default: 'true', description: 'Emit only Long signals (ignore downtrends)' },
    ],
  },
  {
    name: 'atr-sizer',
    role: 'sizer',
    description: 'Sizes positions using ATR-based volatility normalisation. Each trade risks a fixed fraction of portfolio.',
    params: [
      { key: 'risk_per_trade',   type: 'number', default: '0.01', description: 'Fraction of portfolio to risk per trade (e.g. 0.01 = 1%)' },
      { key: 'max_position_pct', type: 'number', default: '0.05', description: 'Maximum position size as fraction of portfolio (e.g. 0.05 = 5%)' },
      { key: 'atr_period',       type: 'number', default: '14',   description: 'ATR lookback period' },
    ],
  },
  {
    name: 'kelly-sizer',
    role: 'sizer',
    description: 'Sizes positions using a fractional Kelly criterion based on historical win rate and payoff ratio.',
    params: [
      { key: 'kelly_fraction',   type: 'number', default: '0.25', description: 'Fraction of full Kelly to use (0.25 = quarter Kelly)' },
      { key: 'max_position_pct', type: 'number', default: '0.10', description: 'Maximum position size as fraction of portfolio' },
      { key: 'lookback_trades',  type: 'number', default: '50',   description: 'Number of recent trades to estimate win rate and payoff' },
    ],
  },
  {
    name: 'regime',
    role: 'identifier',
    description: 'Filters the universe to symbols whose macro regime (trend/volatility state) supports the strategy.',
    params: [
      { key: 'lookback_days',    type: 'number', default: '60',  description: 'Regime detection window' },
      { key: 'vol_threshold',    type: 'number', default: '0.20', description: 'Annualised volatility cap to include a symbol' },
    ],
  },
];

export function pluginsByRole(role: PluginRole): PluginDef[] {
  return PLUGIN_REGISTRY.filter(p => p.role === role);
}

export function findPlugin(name: string): PluginDef | undefined {
  return PLUGIN_REGISTRY.find(p => p.name === name);
}

/** Merge registry defaults into the current param map for a given plugin. */
export function defaultParamsFor(pluginName: string): Record<string, string> {
  const def = findPlugin(pluginName);
  if (!def) return {};
  return Object.fromEntries(def.params.map(p => [p.key, p.default]));
}
