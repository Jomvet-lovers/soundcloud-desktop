import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useEffect, useState } from 'react';
import { api } from '../../lib/api';
import type { Aura } from '../../lib/aura';
import { Loader2 } from '../../lib/icons';
import { toastWriteError } from '../../lib/write-error';
import { useAuthStore } from '../../stores/auth';

interface FollowBtnProps {
  userUrn: string;
  aura: Aura;
}

export function FollowBtn({ userUrn }: FollowBtnProps) {
  const currentUser = useAuthStore((s) => s.user);
  const qc = useQueryClient();

  const { data: initialFollowing = false, isLoading: isQueryLoading } = useQuery({
    queryKey: ['following', currentUser?.urn, userUrn],
    queryFn: () =>
      api<boolean>(
        `/users/${encodeURIComponent(currentUser!.urn)}/followings/${encodeURIComponent(userUrn)}`,
      ),
    enabled: !!currentUser?.urn && !!userUrn,
  });

  const [following, setFollowing] = useState(false);
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    setFollowing(initialFollowing);
  }, [initialFollowing]);

  const toggle = async () => {
    setLoading(true);
    const next = !following;
    setFollowing(next);
    try {
      await api(`/me/followings/${encodeURIComponent(userUrn)}`, {
        method: next ? 'PUT' : 'DELETE',
      });
      qc.invalidateQueries({ queryKey: ['following', currentUser?.urn, userUrn] });
      qc.invalidateQueries({ queryKey: ['user', userUrn] });
      // The cold `/me/followings` cache has staleTime: Infinity — invalidation
      // is mandatory, or the UI won't show a new follow/unfollow until restart.
      qc.invalidateQueries({ queryKey: ['me', 'followings'] });
    } catch (error) {
      setFollowing(!next);
      toastWriteError(error, "SoundCloud didn't accept the follow");
    } finally {
      setLoading(false);
    }
  };

  const busy = loading || isQueryLoading;

  return (
    <button
      type="button"
      onClick={toggle}
      disabled={busy}
      className={`inline-flex items-center justify-center gap-2 h-9 px-5 rounded-full text-[12px] font-semibold tracking-wide transition-colors cursor-pointer disabled:opacity-60 ${
        following ? 'text-white/80 hover:text-white' : 'pill-primary'
      }`}
      style={{
        background: following ? 'var(--following-chip)' : undefined,
        border: following
          ? '0.5px solid color-mix(in srgb, var(--ov-ink) 12%, transparent)'
          : '0.5px solid color-mix(in srgb, var(--ov-ink) 40%, transparent)',
      }}
    >
      {busy ? <Loader2 size={14} className="animate-spin" /> : following ? 'Following' : 'Follow'}
    </button>
  );
}
