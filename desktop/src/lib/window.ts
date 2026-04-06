import { getCurrentWindow } from '@tauri-apps/api/window';

export async function toggleWindowFullscreen() {
  const currentWindow = getCurrentWindow();
  const isFullscreen = await currentWindow.isFullscreen();
  await currentWindow.setFullscreen(!isFullscreen);
}
