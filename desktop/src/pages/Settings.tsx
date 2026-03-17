import { invoke } from '@tauri-apps/api/core';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { reloadCurrentTrack } from '../lib/audio';
import {
  clearAssetsCache,
  clearCache,
  downloadWallpaper,
  getAssetsCacheSize,
  getCacheSize,
  getWallpaperUrl,
  listWallpapers,
  removeWallpaper,
  saveWallpaperFromBuffer,
} from '../lib/cache';
import { Globe, Link, Loader2, Trash2, X } from '../lib/icons';
import { useAuthStore } from '../stores/auth';
import { useSettingsStore } from '../stores/settings';

function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

const PRESET_COLORS = [
  '#ff5500',
  '#ff3366',
  '#7c3aed',
  '#3b82f6',
  '#06b6d4',
  '#10b981',
  '#eab308',
  '#ef4444',
  '#f97316',
  '#8b5cf6',
];

const LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'ru', label: 'Русский' },
] as const;

/* ── Language Section ─────────────────────────────────────── */

const LanguageSection = React.memo(function LanguageSection() {
  const { t, i18n } = useTranslation();

  return (
    <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl">
      <h3 className="text-[15px] font-bold text-white/80 tracking-tight mb-4">
        {t('settings.language')}
      </h3>
      <div className="flex gap-2">
        {LANGUAGES.map((lang) => (
          <button
            key={lang.code}
            onClick={() => i18n.changeLanguage(lang.code)}
            className={`flex items-center gap-2 px-4 py-2.5 rounded-xl text-[13px] font-semibold transition-all duration-200 cursor-pointer border ${
              i18n.language === lang.code
                ? 'bg-white/[0.1] text-white/90 border-white/[0.15]'
                : 'bg-white/[0.02] text-white/40 border-white/[0.05] hover:bg-white/[0.06] hover:text-white/60'
            }`}
          >
            <Globe size={14} strokeWidth={1.8} />
            {lang.label}
          </button>
        ))}
      </div>
    </section>
  );
});

/* ── Cache Section ──────────────────────────────────────── */

function CacheRow({
  label,
  size,
  clearing,
  onClear,
  t,
}: {
  label: string;
  size: number | null;
  clearing: boolean;
  onClear: () => void;
  t: (k: string) => string;
}) {
  return (
    <div className="flex items-center justify-between py-3">
      <div className="flex items-center gap-4">
        <div>
          <p className="text-[13px] text-white/60 font-medium">{label}</p>
          <p className="text-[17px] font-bold text-white/90 tabular-nums mt-0.5">
            {size === null ? (
              <Loader2 size={14} className="animate-spin text-white/20" />
            ) : (
              formatBytes(size)
            )}
          </p>
        </div>
      </div>
      <button
        onClick={onClear}
        disabled={clearing || size === 0}
        className="flex items-center gap-2 px-4 py-2 rounded-xl text-[12px] font-semibold bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/10 hover:border-red-500/20 transition-all duration-300 disabled:opacity-30 cursor-pointer disabled:cursor-not-allowed"
      >
        {clearing ? <Loader2 size={12} className="animate-spin" /> : <Trash2 size={12} />}
        {t('settings.clearCache')}
      </button>
    </div>
  );
}

const CacheSection = React.memo(function CacheSection() {
  const { t } = useTranslation();
  const [audioSize, setAudioSize] = useState<number | null>(null);
  const [assetsSize, setAssetsSize] = useState<number | null>(null);
  const [clearingAudio, setClearingAudio] = useState(false);
  const [clearingAssets, setClearingAssets] = useState(false);

  useEffect(() => {
    getCacheSize().then(setAudioSize);
    getAssetsCacheSize().then(setAssetsSize);
  }, []);

  const handleClearAudio = useCallback(async () => {
    setClearingAudio(true);
    try {
      await clearCache();
      setAudioSize(0);
      toast.success(t('settings.cacheCleared'));
    } catch {
      toast.error(t('common.error'));
    } finally {
      setClearingAudio(false);
    }
  }, [t]);

  const handleClearAssets = useCallback(async () => {
    setClearingAssets(true);
    try {
      await clearAssetsCache();
      setAssetsSize(0);
      toast.success(t('settings.cacheCleared'));
    } catch {
      toast.error(t('common.error'));
    } finally {
      setClearingAssets(false);
    }
  }, [t]);

  const totalSize = (audioSize ?? 0) + (assetsSize ?? 0);

  return (
    <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl space-y-2">
      <div className="flex items-center justify-between mb-2">
        <h3 className="text-[15px] font-bold text-white/80 tracking-tight">
          {t('settings.cache')}
        </h3>
        {audioSize !== null && assetsSize !== null && (
          <span className="text-[12px] text-white/30 tabular-nums">
            {t('settings.total')}: {formatBytes(totalSize)}
          </span>
        )}
      </div>
      <CacheRow
        label={t('settings.audioCacheSize')}
        size={audioSize}
        clearing={clearingAudio}
        onClear={handleClearAudio}
        t={t}
      />
      <div className="border-t border-white/[0.04]" />
      <CacheRow
        label={t('settings.assetsCacheSize')}
        size={assetsSize}
        clearing={clearingAssets}
        onClear={handleClearAssets}
        t={t}
      />
    </section>
  );
});

/* ── Wallpaper Picker ───────────────────────────────────── */

const WallpaperPicker = React.memo(function WallpaperPicker() {
  const { t } = useTranslation();
  const backgroundImage = useSettingsStore((s) => s.backgroundImage);
  const setBackgroundImage = useSettingsStore((s) => s.setBackgroundImage);

  const [wallpapers, setWallpapers] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [downloading, setDownloading] = useState(false);
  const [urlInput, setUrlInput] = useState('');
  const [showUrlInput, setShowUrlInput] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    listWallpapers().then((names) => {
      setWallpapers(names);
      setLoading(false);
    });
  }, []);

  const handleFileSelect = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const file = e.target.files?.[0];
      if (!file) return;
      try {
        const buffer = await file.arrayBuffer();
        const name = await saveWallpaperFromBuffer(buffer, file.name);
        setWallpapers((prev) => [...prev, name]);
        setBackgroundImage(name);
        toast.success(t('settings.wallpaperAdded'));
      } catch {
        toast.error(t('common.error'));
      }
      e.target.value = '';
    },
    [setBackgroundImage, t],
  );

  const handleDownloadUrl = useCallback(async () => {
    const url = urlInput.trim();
    if (!url) return;
    setDownloading(true);
    try {
      const name = await downloadWallpaper(url);
      setWallpapers((prev) => [...prev, name]);
      setBackgroundImage(name);
      setUrlInput('');
      setShowUrlInput(false);
      toast.success(t('settings.wallpaperAdded'));
    } catch {
      toast.error(t('settings.bgLoadError'));
    } finally {
      setDownloading(false);
    }
  }, [urlInput, setBackgroundImage, t]);

  const handleRemove = useCallback(
    async (name: string) => {
      await removeWallpaper(name);
      setWallpapers((prev) => prev.filter((w) => w !== name));
      if (backgroundImage === name) {
        setBackgroundImage('');
      }
    },
    [backgroundImage, setBackgroundImage],
  );

  const handleSelect = useCallback(
    (name: string) => {
      setBackgroundImage(backgroundImage === name ? '' : name);
    },
    [backgroundImage, setBackgroundImage],
  );

  return (
    <div className="space-y-3">
      <label className="text-[13px] text-white/50 font-medium">
        {t('settings.backgroundImage')}
      </label>

      {/* Wallpaper grid */}
      <div className="flex flex-wrap gap-3">
        {/* "None" option */}
        <button
          onClick={() => setBackgroundImage('')}
          className={`w-20 h-14 rounded-xl border-2 transition-all duration-200 cursor-pointer flex items-center justify-center ${
            !backgroundImage
              ? 'border-white/40 bg-white/[0.08]'
              : 'border-white/[0.06] bg-white/[0.02] hover:border-white/[0.12]'
          }`}
        >
          <span className="text-[10px] text-white/40 font-semibold">{t('settings.none')}</span>
        </button>

        {/* Saved wallpapers */}
        {wallpapers.map((name) => {
          const url = getWallpaperUrl(name);
          return (
            <div
              key={name}
              className={`relative group w-20 h-14 rounded-xl overflow-hidden border-2 transition-all duration-200 cursor-pointer ${
                backgroundImage === name
                  ? 'border-white/40 shadow-[0_0_12px_rgba(255,255,255,0.1)]'
                  : 'border-white/[0.06] hover:border-white/[0.15]'
              }`}
              onClick={() => handleSelect(name)}
            >
              {url && <img src={url} alt="" className="w-full h-full object-cover" />}
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  handleRemove(name);
                }}
                className="absolute top-0.5 right-0.5 w-4 h-4 rounded-full bg-black/60 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer hover:bg-red-500/80"
              >
                <X size={8} className="text-white" />
              </button>
              {backgroundImage === name && (
                <div className="absolute inset-0 bg-white/10 flex items-center justify-center">
                  <div className="w-4 h-4 rounded-full bg-white shadow-lg" />
                </div>
              )}
            </div>
          );
        })}

        {loading && (
          <div className="w-20 h-14 rounded-xl bg-white/[0.02] border border-white/[0.06] flex items-center justify-center">
            <Loader2 size={14} className="animate-spin text-white/20" />
          </div>
        )}

        {/* Add from file */}
        <button
          onClick={() => fileInputRef.current?.click()}
          className="w-20 h-14 rounded-xl border-2 border-dashed border-white/[0.1] hover:border-white/[0.2] transition-all cursor-pointer flex flex-col items-center justify-center gap-0.5 hover:bg-white/[0.02]"
        >
          <span className="text-[14px] text-white/30 font-light leading-none">+</span>
          <span className="text-[9px] text-white/25 font-medium">{t('settings.addFile')}</span>
        </button>
        <input
          ref={fileInputRef}
          type="file"
          accept="image/*"
          onChange={handleFileSelect}
          className="hidden"
        />

        {/* Add from URL */}
        <button
          onClick={() => setShowUrlInput(!showUrlInput)}
          className={`w-20 h-14 rounded-xl border-2 border-dashed transition-all cursor-pointer flex flex-col items-center justify-center gap-0.5 ${
            showUrlInput
              ? 'border-white/[0.2] bg-white/[0.04]'
              : 'border-white/[0.1] hover:border-white/[0.2] hover:bg-white/[0.02]'
          }`}
        >
          <Link size={12} className="text-white/30" />
          <span className="text-[9px] text-white/25 font-medium">URL</span>
        </button>
      </div>

      {/* URL download input */}
      {showUrlInput && (
        <div className="flex gap-2 animate-fade-in-up">
          <input
            type="text"
            value={urlInput}
            onChange={(e) => setUrlInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleDownloadUrl()}
            placeholder={t('settings.bgUrlPlaceholder')}
            className="flex-1 px-4 py-2.5 rounded-xl bg-white/[0.04] border border-white/[0.06] text-[13px] text-white/80 placeholder:text-white/20 focus:border-white/[0.12] focus:bg-white/[0.06] transition-all duration-200 outline-none"
            autoFocus
          />
          <button
            onClick={handleDownloadUrl}
            disabled={downloading || !urlInput.trim()}
            className="px-4 py-2.5 rounded-xl text-[12px] font-semibold bg-white/[0.08] text-white/70 hover:bg-white/[0.12] border border-white/[0.06] transition-all disabled:opacity-30 cursor-pointer disabled:cursor-not-allowed"
          >
            {downloading ? <Loader2 size={14} className="animate-spin" /> : t('settings.download')}
          </button>
        </div>
      )}
    </div>
  );
});

/* ── Theme Section ──────────────────────────────────────── */

const ThemeSection = React.memo(function ThemeSection() {
  const { t } = useTranslation();
  const accentColor = useSettingsStore((s) => s.accentColor);
  const backgroundImage = useSettingsStore((s) => s.backgroundImage);
  const backgroundOpacity = useSettingsStore((s) => s.backgroundOpacity);
  const setAccentColor = useSettingsStore((s) => s.setAccentColor);
  const setBackgroundOpacity = useSettingsStore((s) => s.setBackgroundOpacity);
  const resetTheme = useSettingsStore((s) => s.resetTheme);

  const colorInputRef = useRef<HTMLInputElement>(null);

  return (
    <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl space-y-6">
      <div className="flex items-center justify-between">
        <h3 className="text-[15px] font-bold text-white/80 tracking-tight">
          {t('settings.appearance')}
        </h3>
        <button
          onClick={resetTheme}
          className="text-[12px] text-white/30 hover:text-white/60 transition-colors cursor-pointer"
        >
          {t('settings.resetDefaults')}
        </button>
      </div>

      {/* Accent Color */}
      <div className="space-y-3">
        <label className="text-[13px] text-white/50 font-medium">{t('settings.accentColor')}</label>
        <div className="flex items-center gap-2 flex-wrap">
          {PRESET_COLORS.map((color) => (
            <button
              key={color}
              onClick={() => setAccentColor(color)}
              className="w-8 h-8 rounded-full border-2 transition-all duration-200 cursor-pointer hover:scale-110 active:scale-95 shadow-md"
              style={{
                backgroundColor: color,
                borderColor: accentColor === color ? 'white' : 'transparent',
                boxShadow: accentColor === color ? `0 0 16px ${color}60` : undefined,
              }}
            />
          ))}
          <button
            onClick={() => colorInputRef.current?.click()}
            className="w-8 h-8 rounded-full border-2 border-dashed border-white/20 hover:border-white/40 transition-all cursor-pointer flex items-center justify-center text-white/30 hover:text-white/60 hover:scale-110"
          >
            <span className="text-[11px] font-bold">+</span>
          </button>
          <input
            ref={colorInputRef}
            type="color"
            value={accentColor}
            onChange={(e) => setAccentColor(e.target.value)}
            className="sr-only"
          />
        </div>
      </div>

      {/* Background Image */}
      <WallpaperPicker />

      {/* Background Opacity */}
      {backgroundImage && (
        <div className="space-y-3">
          <div className="flex items-center justify-between">
            <label className="text-[13px] text-white/50 font-medium">
              {t('settings.bgOpacity')}
            </label>
            <span className="text-[12px] text-white/30 tabular-nums">
              {Math.round(backgroundOpacity * 100)}%
            </span>
          </div>
          <input
            type="range"
            min={0}
            max={0.5}
            step={0.01}
            value={backgroundOpacity}
            onChange={(e) => setBackgroundOpacity(Number(e.target.value))}
            className="w-full accent-[var(--color-accent)] h-1 bg-white/10 rounded-full appearance-none cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-lg"
          />
        </div>
      )}
    </section>
  );
});

/* ── Audio Device Section ──────────────────────────────── */

interface AudioSink {
  name: string;
  description: string;
  is_default: boolean;
}

const AudioDeviceSection = React.memo(function AudioDeviceSection() {
  const { t } = useTranslation();
  const [sinks, setSinks] = useState<AudioSink[]>([]);
  const [switching, setSwitching] = useState(false);

  useEffect(() => {
    invoke<AudioSink[]>('audio_list_devices').then(setSinks).catch(console.error);
  }, []);

  const handleSwitch = async (sinkName: string) => {
    const current = sinks.find((s) => s.is_default);
    if (switching || current?.name === sinkName) return;
    setSwitching(true);
    try {
      await invoke('audio_switch_device', { deviceName: sinkName });
      setSinks((prev) => prev.map((s) => ({ ...s, is_default: s.name === sinkName })));
      await reloadCurrentTrack();
      toast.success(t('settings.audioDeviceSwitched'));
    } catch (err) {
      toast.error(String(err));
    } finally {
      setSwitching(false);
    }
  };

  if (sinks.length === 0) return null;

  return (
    <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl">
      <h3 className="text-[15px] font-bold text-white/80 tracking-tight mb-4">
        {t('settings.audioDevice')}
      </h3>
      <div className="flex gap-2 flex-wrap">
        {sinks.map((sink) => (
          <button
            key={sink.name}
            onClick={() => handleSwitch(sink.name)}
            disabled={switching}
            className={`flex items-center gap-2 px-4 py-2.5 rounded-xl text-[13px] font-semibold transition-all duration-200 cursor-pointer border ${
              sink.is_default
                ? 'bg-white/[0.1] text-white/90 border-white/[0.15]'
                : 'bg-white/[0.02] text-white/40 border-white/[0.05] hover:bg-white/[0.06] hover:text-white/60'
            } disabled:opacity-50`}
          >
            {sink.description}
          </button>
        ))}
      </div>
    </section>
  );
});

/* ── Account Section ────────────────────────────────────── */

const AccountSection = React.memo(function AccountSection() {
  const { t } = useTranslation();
  const logout = useAuthStore((s) => s.logout);

  return (
    <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl">
      <h3 className="text-[15px] font-bold text-white/80 tracking-tight mb-5">
        {t('settings.account')}
      </h3>
      <button
        onClick={logout}
        className="flex items-center gap-2 px-5 py-2.5 rounded-xl text-[13px] font-semibold bg-red-500/10 text-red-400 hover:bg-red-500/20 border border-red-500/10 hover:border-red-500/20 transition-all duration-300 cursor-pointer"
      >
        {t('auth.signOut')}
      </button>
    </section>
  );
});

/* ── Main ───────────────────────────────────────────────── */

export function Settings() {
  const { t } = useTranslation();

  return (
    <div className="p-6 pb-4 max-w-2xl mx-auto space-y-6">
      <h1 className="text-3xl font-extrabold text-white tracking-tight">{t('settings.title')}</h1>
      <LanguageSection />
      <CacheSection />
      <ThemeSection />
      <AudioDeviceSection />
      <AccountSection />
    </div>
  );
}
