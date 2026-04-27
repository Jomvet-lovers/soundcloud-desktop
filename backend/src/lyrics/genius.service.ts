import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { firstValueFrom } from 'rxjs';

const GENIUS_SEARCH = 'https://genius.com/api/search/multi';
const TIMEOUT_MS = 15000;
const UA =
  'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36';

export interface GeniusCandidate {
  plainText: string;
  artistGuess?: string;
  titleGuess?: string;
}

interface SearchHit {
  result?: {
    url?: string;
    title?: string;
    primary_artist?: { name?: string };
  };
}

@Injectable()
export class GeniusService {
  private readonly logger = new Logger(GeniusService.name);

  constructor(private readonly http: HttpService) {}

  async searchByQuery(q: string, limit = 3): Promise<GeniusCandidate[]> {
    try {
      const searchUrl = `${GENIUS_SEARCH}?q=${encodeURIComponent(q)}`;
      const searchResp = await firstValueFrom(
        this.http.get<{
          response?: { sections?: { type: string; hits?: SearchHit[] }[] };
        }>(searchUrl, {
          timeout: TIMEOUT_MS,
          headers: { 'User-Agent': UA, Accept: 'application/json' },
        }),
      );

      const sections = searchResp.data?.response?.sections ?? [];
      const hits: SearchHit[] = [];
      for (const section of sections) {
        if (section.type === 'song' && section.hits?.length) hits.push(...section.hits);
      }

      const candidates: GeniusCandidate[] = [];
      for (const hit of hits.slice(0, limit)) {
        const url = hit.result?.url;
        if (!url) continue;
        try {
          const htmlResp = await firstValueFrom(
            this.http.get<string>(url, { timeout: TIMEOUT_MS, responseType: 'text' }),
          );
          const plainText = this.parseLyricsHtml(htmlResp.data);
          if (plainText) {
            candidates.push({
              plainText,
              artistGuess: hit.result?.primary_artist?.name,
              titleGuess: hit.result?.title,
            });
          }
        } catch (e) {
          this.logger.debug(`genius page fetch failed: ${(e as Error).message}`);
        }
      }
      return candidates;
    } catch (e) {
      this.logger.debug(`Genius search failed: ${(e as Error).message}`);
      return [];
    }
  }

  private parseLyricsHtml(html: string): string | null {
    const openRe = /<div\b[^>]*\bdata-lyrics-container="true"[^>]*>/gi;
    const parts: string[] = [];
    let m: RegExpExecArray | null = openRe.exec(html);
    while (m !== null) {
      const inner = this.extractBalancedDivContent(html, m.index + m[0].length);
      if (inner) parts.push(inner);
      m = openRe.exec(html);
    }
    if (!parts.length) return null;

    let text = parts
      .join('\n')
      .replace(/<br\s*\/?>/gi, '\n')
      .replace(/<[^>]+>/g, '')
      .replace(/&nbsp;/g, ' ')
      .replace(/&amp;/g, '&')
      .replace(/&lt;/g, '<')
      .replace(/&gt;/g, '>')
      .replace(/&#x27;|&apos;/g, "'")
      .replace(/&quot;/g, '"');

    text = text
      .replace(/^\d+\s*Contributors/i, '')
      .replace(/^[^\n]*?Lyrics/i, '')
      .replace(/^\[Текст песни.*?\]/i, '')
      .trim();

    return text.length > 20 ? text : null;
  }

  /**
   * Extracts content of a `<div ...>` whose opening tag has already been
   * consumed (caller passes position right after `>`). Walks forward counting
   * nested `<div>` / `</div>` until depth returns to 0. Genius wraps each
   * verse in `<div data-lyrics-container="true">` but those blocks contain
   * nested `<div>` (footer/inline-tooltip/etc). A non-greedy `</div>` stops at
   * the first nested one — truncating the verse to the first chunk.
   */
  private extractBalancedDivContent(html: string, startPos: number): string | null {
    let depth = 1;
    let pos = startPos;
    const len = html.length;
    while (pos < len && depth > 0) {
      const nextOpen = html.indexOf('<div', pos);
      const nextClose = html.indexOf('</div', pos);
      if (nextClose === -1) return null;
      if (nextOpen !== -1 && nextOpen < nextClose) {
        const after = html.charCodeAt(nextOpen + 4);
        if (
          after === 32 ||
          after === 9 ||
          after === 10 ||
          after === 13 ||
          after === 62 ||
          after === 47
        ) {
          depth++;
        }
        pos = nextOpen + 4;
      } else {
        depth--;
        if (depth === 0) return html.slice(startPos, nextClose);
        pos = nextClose + 5;
      }
    }
    return null;
  }
}
