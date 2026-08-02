// «main доступен, но не обслуживает» — состояние, которого у пробы нет.
//
// Вердикт main двигает только `/health`, а эта ручка на бэке дешёвая: она не
// берёт per-session refresh-мьютекс, поэтому отвечает быстро даже когда все
// авторизованные ручки стоят в конвое за ним. Значит `main === 'down'` в самом
// частом инциденте НЕ наступает никогда — хост «жив», просто ничего не успевает.
// А на 'down' завязано всё переключение: control-plane, `/me/subscription`,
// порядок баз премиума. Итог — подписчики продолжали грузить полумёртвый main,
// хотя у них есть star, и конвой на main давил заодно и тех, кому star закрыт.
//
// Здесь копится второй сигнал, по РЕАЛЬНЫМ запросам к main: провал или ответ
// дольше `SLOW_RESPONSE_MS`. Набралось `THRESHOLD` за `WINDOW_MS` → main
// деградировал на `COOLDOWN_MS`. Успех вердикт не снимает: под конвоем часть
// запросов проходит штатно, на то он и конвой. Деградация истекает сама —
// это half-open проверка, и если main всё ещё плох, она набирается снова.

const WINDOW_MS = 60_000;
const THRESHOLD = 3;
const COOLDOWN_MS = 60_000;
/**
 * Сколько после истечения деградации хватает ОДНОГО плохого ответа, чтобы
 * вернуть вердикт. Half-open проверка спрашивает «починили?», и один ответ
 * «нет» — исчерпывающий; иначе каждая минута инцидента стоит премиуму трёх
 * запросов, отправленных обратно в конвой.
 */
const RE_ARM_MS = 120_000;

/**
 * Ответ дольше этого — уже не «медленно», а «не обслуживает». Прод-замеры под
 * конвоем refresh-лока (30.07.2026): `/me/subscription` 17.9 с при том, что
 * сама ручка это один SELECT; `/recommendations` — 49.8 с.
 */
export const SLOW_RESPONSE_MS = 12_000;

let badSamples: number[] = [];
let degradedUntil = 0;

/** main отвечает, но обслуживать не может — премиуму пора на star. */
export function isMainDegraded(): boolean {
  return Date.now() < degradedUntil;
}

/**
 * Вердикт без набора окна — для доказательства, а не для сэмпла. Единственный
 * такой случай: star отдал то, в чём main отказал, при общей на два хоста
 * сессии. Это не «медленно» и не «не повезло», это подтверждённая поломка
 * main, и ждать ещё двух сэмплов незачем.
 */
export function markMainDegraded(): void {
  badSamples = [];
  setDegradedUntil(Date.now() + COOLDOWN_MS);
}

/**
 * Реальный запрос к main провалился или тащился дольше `SLOW_RESPONSE_MS`.
 * Что считать плохим ответом, решает вызывающий: он один знает путь, а у
 * `/auth/*` долгий ответ — это round-trip бэка в SoundCloud, а не деградация.
 */
export function noteMainBadResponse(): void {
  const now = Date.now();
  badSamples = badSamples.filter((t) => now - t < WINDOW_MS);
  badSamples.push(now);
  if (badSamples.length < currentThreshold(now)) return;
  // Окно набрано — считаем заново, чтобы старые сэмплы не продлевали вердикт
  // задним числом после того, как он уже отработал.
  badSamples = [];
  setDegradedUntil(now + COOLDOWN_MS);
}

/** Пока деградация свежа (идёт или только что истекла) — порог понижен. */
function currentThreshold(now: number): number {
  return now - degradedUntil < RE_ARM_MS ? 1 : THRESHOLD;
}

// ─── Подписка для UI ────────────────────────────────────────
// Юзеру нельзя молча переехать на резервный хост: «пусто и ничего не сказали»
// неотличимо от сломанного приложения. Баннер читает вердикт отсюда.

const listeners = new Set<() => void>();
let expiryTimer: ReturnType<typeof setTimeout> | null = null;

function setDegradedUntil(ts: number): void {
  degradedUntil = ts;
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  // Деградация кончается по часам, а не по событию: без будильника подписчик
  // узнал бы об этом только со следующим запросом — то есть, может, никогда.
  expiryTimer = setTimeout(
    () => {
      expiryTimer = null;
      emit();
    },
    Math.max(0, ts - Date.now()),
  );
  emit();
}

function emit(): void {
  for (const l of listeners) l();
}

export function subscribeMainDegraded(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
