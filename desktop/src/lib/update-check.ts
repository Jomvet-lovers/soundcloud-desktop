import { fetch } from '@tauri-apps/plugin-http';
import i18n from '../i18n';
import { APP_VERSION, GITHUB_OWNER, GITHUB_REPO, GITHUB_REPO_EN } from './constants';

export interface GithubRelease {
  tag_name: string;
  name: string;
  body: string;
  html_url: string;
  published_at: string;
}

function stripLeadingV(version: string) {
  return version.replace(/^v/, '');
}

async function fetchRelease(repo: string): Promise<GithubRelease | null> {
  const url = `https://api.github.com/repos/${GITHUB_OWNER}/${repo}/releases/latest`;
  const response = await fetch(url);
  return response.ok ? response.json() : null;
}

export async function checkForAppUpdate(): Promise<GithubRelease | null> {
  const primaryRelease = await fetchRelease(GITHUB_REPO).catch(() => null);
  if (!primaryRelease) return null;

  const latest = stripLeadingV(primaryRelease.tag_name);
  const current = stripLeadingV(APP_VERSION);
  if (latest === current) return null;

  const prefersEnglishRelease = !i18n.language?.startsWith('ru');
  if (prefersEnglishRelease) {
    const englishRelease = await fetchRelease(GITHUB_REPO_EN).catch(() => null);
    if (englishRelease && stripLeadingV(englishRelease.tag_name) === latest) {
      return englishRelease;
    }
  }

  return primaryRelease;
}
