import { fetch } from '@tauri-apps/plugin-http';
import { APP_VERSION, GITHUB_OWNER, GITHUB_REPO } from './constants';
import { isNewerVersion } from './semver';

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

/** Fork tags look like `v8.4.13-direct.1`: strip the `-direct.N` build suffix
 *  so the base semver can be compared. Returns [base, directBuild]. */
function splitDirectTag(version: string): [string, number] {
  const m = /^(.*?)(?:-direct\.(\d+))?$/.exec(version);
  return [m?.[1] ?? version, m?.[2] === undefined ? 0 : Number.parseInt(m[2], 10)];
}

async function fetchRelease(repo: string): Promise<GithubRelease | null> {
  const url = `https://api.github.com/repos/${GITHUB_OWNER}/${repo}/releases/latest`;
  const response = await fetch(url);
  return response.ok ? response.json() : null;
}

export async function checkForAppUpdate(): Promise<GithubRelease | null> {
  const primaryRelease = await fetchRelease(GITHUB_REPO).catch(() => null);
  if (!primaryRelease) return null;

  if (!isNewerDirectRelease(primaryRelease.tag_name, currentFullVersion())) return null;

  return primaryRelease;
}

/**
 * The running app's full version. Release builds are stamped with their own
 * tag (SCD_RELEASE_TAG); dev/unstamped builds only know the bare package
 * version. A bare version compares equal to any -direct build of the same
 * base, so dev never nags about fork rebuilds — only a newer base does.
 */
export function currentFullVersion(): string {
  const tag = stripLeadingV(__DIRECT_TAG__ || '');
  if (tag) return tag;
  return stripLeadingV(APP_VERSION);
}

function isNewerDirectRelease(latestTag: string, currentTag: string): boolean {
  const [latestBase, latestBuild] = splitDirectTag(stripLeadingV(latestTag));
  const [currentBase, currentBuild] = splitDirectTag(stripLeadingV(currentTag));
  if (isNewerVersion(latestBase, currentBase)) return true;
  if (isNewerVersion(currentBase, latestBase)) return false;
  // Same base: an unstamped (dev) build never counts as older than a
  // -direct rebuild of the same base.
  if (!/-direct\.\d+$/.test(currentTag)) return false;
  return latestBuild > currentBuild;
}
