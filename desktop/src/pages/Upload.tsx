import { UploadArtwork } from '../components/upload/UploadArtwork';
import { UploadAudio } from '../components/upload/UploadAudio';
import { UploadForm } from '../components/upload/UploadForm';
import { UploadProgress } from '../components/upload/UploadProgress';
import { UploadResult } from '../components/upload/UploadResult';
import { useUploadFlow } from '../components/upload/useUploadFlow';

/** Upload — pick an audio file, fill in metadata, upload it to SoundCloud. */
export function Upload() {
  const flow = useUploadFlow();
  const {
    audio,
    inspecting,
    inspectError,
    artwork,
    previewFailed,
    meta,
    phase,
    progress,
    cancelling,
    failure,
    result,
    uploading,
    blocked,
    pickAudio,
    pickArtwork,
    handleSubmit,
    handleCancel,
    resetForNext,
    patchMeta,
    clearArtwork,
    markPreviewFailed,
  } = flow;

  return (
    <div className="relative w-full min-h-screen">
      <div
        className="relative z-10 w-full max-w-[1480px] mx-auto px-4 md:px-8 pt-6 md:pt-8 pb-32"
        style={{ isolation: 'isolate' }}
      >
        <header className="mb-7">
          <h1 className="text-[24px] font-semibold tracking-tight text-white/92">{'Upload'}</h1>
          <p className="mt-1 text-[12.5px] text-white/40">
            {'Upload a track to SoundCloud with artwork and track details.'}
          </p>
        </header>

        <div className="flex w-full max-w-[720px] flex-col gap-8">
          <UploadAudio
            audio={audio}
            inspecting={inspecting}
            inspectError={inspectError}
            disabled={uploading}
            onPick={() => void pickAudio()}
          />

          {audio &&
            (uploading ? (
              <section className="flex flex-col gap-4 border-t border-white/[0.06] pt-6">
                <h2 className="text-[15px] font-semibold text-white/90">{'Uploading'}</h2>
                <UploadProgress
                  percent={progress}
                  cancelling={cancelling}
                  onCancel={handleCancel}
                />
              </section>
            ) : phase === 'done' && result ? (
              <UploadResult result={result} onReset={resetForNext} />
            ) : (
              <section className="flex flex-col gap-5 border-t border-white/[0.06] pt-6">
                <h2 className="text-[15px] font-semibold text-white/90">{'Track details'}</h2>
                {failure && (
                  <p role="alert" className="text-[12.5px] leading-snug text-red-400">
                    {failure}
                  </p>
                )}
                {phase === 'cancelled' && !failure && (
                  <p className="text-[12.5px] leading-snug text-white/55">{'Upload cancelled.'}</p>
                )}
                <UploadForm
                  value={meta}
                  onChange={patchMeta}
                  disabled={blocked}
                  onSubmit={() => void handleSubmit()}
                >
                  <UploadArtwork
                    artwork={artwork}
                    previewFailed={previewFailed}
                    onPick={() => void pickArtwork()}
                    onRemove={clearArtwork}
                    onPreviewError={markPreviewFailed}
                  />
                </UploadForm>
              </section>
            ))}
        </div>
      </div>
    </div>
  );
}
