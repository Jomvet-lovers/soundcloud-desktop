import {AlertCircle, Link2, Loader2} from 'lucide-react';
import {memo, useEffect, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {useNavigate} from 'react-router-dom';
import {resolveTrackFromStreaming} from '../../lib/streaming';

interface ResolveCardProps {
    url: string;
    onDone: () => void;
}

/* SoundCloud URL short-circuit: resolve the link and redirect to the track. */
export const ResolveCard = memo(function ResolveCard({url, onDone}: ResolveCardProps) {
    const {t} = useTranslation();
    const navigate = useNavigate();
    const [error, setError] = useState(false);

    useEffect(() => {
        let cancelled = false;
        setError(false);
        resolveTrackFromStreaming(url)
            .then((resolved) => {
                if (cancelled) return;
                if (resolved?.urn) {
                    navigate(`/track/${encodeURIComponent(resolved.urn)}`);
                    onDone();
                } else {
                    setError(true);
                }
            })
            .catch(() => {
                if (!cancelled) setError(true);
            });
        return () => {
            cancelled = true;
        };
    }, [url, navigate, onDone]);

    return (
        <div className="flex justify-center px-4 pt-16">
            <div
                className="w-full max-w-[440px] flex flex-col items-center gap-4 p-8 rounded-[2rem] text-center"
                style={{
                    background: 'linear-gradient(165deg, rgba(255,255,255,0.06), rgba(255,255,255,0.02))',
                    border: '0.5px solid rgba(255,255,255,0.1)',
                    backdropFilter: 'blur(40px) saturate(160%)',
                    WebkitBackdropFilter: 'blur(40px) saturate(160%)',
                    boxShadow: '0 30px 80px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.06)',
                    isolation: 'isolate',
                }}
            >
                <div
                    className="w-14 h-14 flex items-center justify-center rounded-2xl"
                    style={{
                        background: 'rgba(255,255,255,0.04)',
                        border: '0.5px solid rgba(255,255,255,0.08)',
                    }}
                >
                    {error ? (
                        <AlertCircle size={24} className="text-white/50"/>
                    ) : (
                        <Loader2 size={24} className="text-accent animate-spin"/>
                    )}
                </div>
                <div className="flex flex-col gap-1">
                    <p className="text-[15px] font-semibold text-white/90">
                        {error ? t('search.resolve.errorTitle') : t('search.resolve.loading')}
                    </p>
                    <p className="flex items-center justify-center gap-1.5 text-[12px] text-white/40 font-mono">
                        <Link2 size={12}/>
                        <span className="max-w-[300px] truncate">{url}</span>
                    </p>
                </div>
                {error && (
                    <button
                        type="button"
                        onClick={onDone}
                        className="h-9 px-5 rounded-full text-[13px] text-white/80 cursor-pointer transition-colors hover:bg-white/[0.06]"
                        style={{border: '0.5px solid rgba(255,255,255,0.12)'}}
                    >
                        {t('search.back')}
                    </button>
                )}
            </div>
        </div>
    );
});
