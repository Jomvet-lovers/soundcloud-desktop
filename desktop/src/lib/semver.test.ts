import { describe, expect, it } from 'vitest';
import { isNewerVersion } from './semver';

describe('isNewerVersion', () => {
  it('returns true when latest patch is greater', () => {
    expect(isNewerVersion('7.0.1', '7.0.0')).toBe(true);
  });

  it('returns true when latest minor is greater', () => {
    expect(isNewerVersion('7.1.0', '7.0.9')).toBe(true);
  });

  it('returns true when latest major is greater', () => {
    expect(isNewerVersion('8.0.0', '7.9.9')).toBe(true);
  });

  it('returns false when versions are equal', () => {
    expect(isNewerVersion('7.0.1', '7.0.1')).toBe(false);
  });

  it('returns false when current is newer than latest patch', () => {
    expect(isNewerVersion('7.0.0', '7.0.1')).toBe(false);
  });

  it('returns false when current is newer than latest minor', () => {
    expect(isNewerVersion('7.0.9', '7.1.0')).toBe(false);
  });

  it('returns false when current is newer than latest major', () => {
    expect(isNewerVersion('7.9.9', '8.0.0')).toBe(false);
  });

  it('treats missing trailing segments as zero', () => {
    expect(isNewerVersion('7.0', '7.0.0')).toBe(false);
    expect(isNewerVersion('7.0.0', '7.0')).toBe(false);
    expect(isNewerVersion('7.1', '7.0.5')).toBe(true);
  });

  it('compares numerically, not lexicographically', () => {
    expect(isNewerVersion('1.10.0', '1.9.0')).toBe(true);
    expect(isNewerVersion('1.9.0', '1.10.0')).toBe(false);
  });

  it('returns false for non-numeric versions to avoid false positives', () => {
    expect(isNewerVersion('7.0.0-beta', '7.0.0')).toBe(false);
    expect(isNewerVersion('latest', '7.0.0')).toBe(false);
    expect(isNewerVersion('7.0.0', '')).toBe(false);
  });
});
