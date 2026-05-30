import {useMemo} from 'react';
import {useSettingsStore} from '../stores/settings';
import {type Aura, auraFromHex, DEFAULT_AURA} from './aura';

/**
 * Аура по умолчанию для профилей без «звезды»: строится из акцентного цвета
 * смотрящего (текущего юзера), чтобы дефолтный профиль совпадал с его темой.
 * Профили со «звездой» используют выбранный владельцем цвет — этот хук там
 * не участвует.
 */
export function useViewerAura(): Aura {
    const accentColor = useSettingsStore((s) => s.accentColor);
    return useMemo(() => auraFromHex(accentColor) ?? DEFAULT_AURA, [accentColor]);
}
