import { memo } from 'react';
import { LIBRARY_KEYFRAMES } from '../library/keyframes';
import { USER_PAGE_KEYFRAMES } from '../user/keyframes';
import { AUTH_KEYFRAMES } from './auth-keyframes';

/** Flat login backdrop: keeps the shared keyframes, no decorative layers. */
export const AuthBackdrop = memo(function AuthBackdrop() {
  return <style>{LIBRARY_KEYFRAMES + USER_PAGE_KEYFRAMES + AUTH_KEYFRAMES}</style>;
});
