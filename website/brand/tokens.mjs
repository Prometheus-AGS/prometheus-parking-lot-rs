// know-me UI/UX standard, section 4.1 color system (dark and light).
// Single source for the verify script's brand-fidelity check. The CSS custom
// properties in src/css/custom.css must resolve to exactly these values.
export const tokens = {
  dark: {
    canvas: '#0B0F14',
    chrome: '#111620',
    surface: '#161D29',
    raised: '#1C2535',
    hover: '#202B40',
    muted: '#253044',
    text: '#E8EDF3',
    textSecondary: '#A7B0BC',
    textFaint: '#6B7280',
    ember: '#FF6A3D',
    cyan: '#00C2DC',
    success: '#22C55E',
    warning: '#F59E0B',
    error: '#EF4444',
  },
  light: {
    canvas: '#F7F7F8',
    chrome: '#FFFFFF',
    surface: '#FAFBFC',
    raised: '#FFFFFF',
    hover: '#F2F4F7',
    muted: '#EEF0F3',
    text: '#0B0F14',
    textSecondary: '#4B5563',
    textFaint: '#6B7280',
    ember: '#E04E28',
    cyan: '#0891B2',
    success: '#16A34A',
    warning: '#D97706',
    error: '#DC2626',
  },
};

// Token name -> CSS custom property.
export const cssVar = {
  canvas: '--pl-canvas',
  chrome: '--pl-chrome',
  surface: '--pl-surface',
  raised: '--pl-raised',
  hover: '--pl-hover',
  muted: '--pl-muted',
  text: '--pl-text',
  textSecondary: '--pl-text-secondary',
  textFaint: '--pl-text-faint',
  ember: '--pl-ember',
  cyan: '--pl-cyan',
  success: '--pl-success',
  warning: '--pl-warning',
  error: '--pl-error',
};
