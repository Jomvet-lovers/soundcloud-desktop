import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Animated, Pressable, ScrollView, StyleSheet, TextInput, View } from 'react-native';
import { type PlaylistSummary, useSc } from '@sc/data';
import { CheckIcon, CloseIcon, Cover, GlassSurface, ListPlusIcon, modalGlass, ScText, useScTheme } from '@sc/ui';
import { playlistDialog, useAddToPlaylistTarget } from '../player/playlist-dialog';

const CARD_WIDTH = 460;

/** «Добавить в плейлист» (донор `AddToPlaylistDialog`): центр-модалка со списком
 *  своих плейлистов + создание нового. Цель ставит `playlistDialog.open(track)`. */
export function AddToPlaylistDialog() {
  const track = useAddToPlaylistTarget();
  const sc = useSc();
  const { accent } = useScTheme();
  const [playlists, setPlaylists] = useState<PlaylistSummary[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null); // id плейлиста в процессе / 'new'
  const [done, setDone] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const anim = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    if (!track) return;
    setPlaylists(null);
    setDone(null);
    setTitle('');
    anim.setValue(0);
    Animated.timing(anim, { toValue: 1, duration: 200, useNativeDriver: true }).start();
    let alive = true;
    void sc.library
      .playlists(100, 0)
      .then((p) => { if (alive) setPlaylists(p.items.filter((x) => !x.is_album)); })
      .catch(() => { if (alive) setPlaylists([]); });
    return () => { alive = false; };
  }, [track, sc, anim]);

  if (!track) return null;

  const addTo = (pl: PlaylistSummary) => {
    setBusy(pl.id);
    void sc.playlists
      .addTrack(pl.id, track.id)
      .then(() => {
        setDone(pl.id);
        setTimeout(() => playlistDialog.close(), 650);
      })
      .catch(() => setBusy(null));
  };

  const createNew = () => {
    const name = title.trim();
    if (!name) return;
    setBusy('new');
    void sc.playlists
      .create(name, [track.id])
      .then(() => {
        setDone('new');
        setTimeout(() => playlistDialog.close(), 650);
      })
      .catch(() => setBusy(null));
  };

  const translateY = anim.interpolate({ inputRange: [0, 1], outputRange: [8, 0] });

  return (
    <View style={[StyleSheet.absoluteFill, { zIndex: 95 }]}>
      <Pressable onPress={() => playlistDialog.close()} style={[StyleSheet.absoluteFill, { backgroundColor: 'rgba(0,0,0,0.5)' }]} />
      <View pointerEvents="box-none" style={[StyleSheet.absoluteFill, { alignItems: 'center', justifyContent: 'center' }]}>
        <Animated.View style={{ width: CARD_WIDTH, maxWidth: '92%', opacity: anim, transform: [{ translateY }] }}>
          <GlassSurface recipe={modalGlass}>
            <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'space-between', paddingHorizontal: 22, paddingTop: 18, paddingBottom: 12 }}>
              <View style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
                <ListPlusIcon size={18} color="rgba(255,255,255,0.6)" />
                <ScText style={{ fontSize: 16, fontWeight: '700', color: 'rgba(255,255,255,0.92)' }}>Добавить в плейлист</ScText>
              </View>
              <Pressable onPress={() => playlistDialog.close()} style={{ width: 32, height: 32, borderRadius: 10, alignItems: 'center', justifyContent: 'center' }}>
                <CloseIcon size={16} color="rgba(255,255,255,0.4)" />
              </Pressable>
            </View>

            <ScText numberOfLines={1} style={{ paddingHorizontal: 22, marginBottom: 10, fontSize: 12.5, color: 'rgba(255,255,255,0.45)' }}>
              {track.title} · {track.artist.name}
            </ScText>

            {/* создать новый */}
            <View style={{ flexDirection: 'row', gap: 8, paddingHorizontal: 22, marginBottom: 12 }}>
              <TextInput
                value={title}
                onChangeText={setTitle}
                placeholder="Новый плейлист…"
                placeholderTextColor="rgba(255,255,255,0.3)"
                onSubmitEditing={createNew}
                style={{ flex: 1, height: 38, borderRadius: 10, paddingHorizontal: 12, color: '#fff', fontSize: 13, backgroundColor: 'rgba(255,255,255,0.05)', borderWidth: 1, borderColor: 'rgba(255,255,255,0.08)' }}
              />
              <Pressable
                onPress={createNew}
                disabled={!title.trim() || busy === 'new'}
                style={{ height: 38, paddingHorizontal: 14, borderRadius: 10, alignItems: 'center', justifyContent: 'center', backgroundColor: title.trim() ? accent.base : 'rgba(255,255,255,0.06)' }}
              >
                {done === 'new' ? <CheckIcon size={16} color="#fff" /> : <ScText style={{ fontSize: 13, fontWeight: '600', color: title.trim() ? accent.contrast : 'rgba(255,255,255,0.4)' }}>Создать</ScText>}
              </Pressable>
            </View>

            <View style={{ height: 1, backgroundColor: 'rgba(255,255,255,0.06)' }} />

            <ScrollView style={{ maxHeight: 320 }} contentContainerStyle={{ paddingVertical: 6 }}>
              {playlists == null ? (
                <View style={{ paddingVertical: 40, alignItems: 'center' }}>
                  <ActivityIndicator color="rgba(255,255,255,0.5)" />
                </View>
              ) : playlists.length === 0 ? (
                <ScText style={{ paddingVertical: 34, textAlign: 'center', fontSize: 13, color: 'rgba(255,255,255,0.4)' }}>
                  Своих плейлистов пока нет
                </ScText>
              ) : (
                playlists.map((pl) => (
                  <Pressable
                    key={pl.id}
                    onPress={() => addTo(pl)}
                    disabled={busy != null}
                    style={{ flexDirection: 'row', alignItems: 'center', gap: 12, paddingHorizontal: 22, paddingVertical: 8 }}
                  >
                    <Cover url={pl.artwork_url} size={40} radius={8} artSize="t200x200" />
                    <View style={{ flex: 1, minWidth: 0 }}>
                      <ScText numberOfLines={1} style={{ fontSize: 13.5, fontWeight: '500', color: 'rgba(255,255,255,0.9)' }}>{pl.title}</ScText>
                      <ScText style={{ fontSize: 11.5, color: 'rgba(255,255,255,0.4)' }}>{pl.track_count} треков</ScText>
                    </View>
                    {done === pl.id ? (
                      <CheckIcon size={18} color={accent.base} />
                    ) : busy === pl.id ? (
                      <ActivityIndicator size="small" color="rgba(255,255,255,0.5)" />
                    ) : null}
                  </Pressable>
                ))
              )}
            </ScrollView>
            <View style={{ height: 12 }} />
          </GlassSurface>
        </Animated.View>
      </View>
    </View>
  );
}
