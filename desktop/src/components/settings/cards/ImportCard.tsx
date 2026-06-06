import React, {useState} from 'react';
import {useTranslation} from 'react-i18next';
import {Download} from '../../../lib/icons';
import {Card} from '../primitives';

const YMImportDialogLazy = React.lazy(() => import('../../music/YMImportDialog'));

export function ImportCard() {
    const {t} = useTranslation();
    const [ymOpen, setYmOpen] = useState(false);

    return (
        <Card title={t('settings.import')} icon={<Download size={17}/>}>
            <button
                type="button"
                onClick={() => setYmOpen(true)}
                className="flex items-center gap-2 px-5 py-2.5 rounded-xl text-[13px] font-semibold bg-white/[0.06] text-white/70 hover:bg-white/[0.1] border border-white/[0.06] hover:border-white/[0.12] transition-all duration-300 cursor-pointer"
            >
                {t('settings.importYandex')}
            </button>
            {ymOpen && (
                <React.Suspense fallback={null}>
                    <YMImportDialogLazy open={ymOpen} onOpenChange={setYmOpen}/>
                </React.Suspense>
            )}
        </Card>
    );
}
