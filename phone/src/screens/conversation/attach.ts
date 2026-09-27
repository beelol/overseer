import { ImageManipulator, SaveFormat } from 'expo-image-manipulator';
import * as ImagePicker from 'expo-image-picker';

/** An image that goes with a message, already made small enough to send. */
export interface Attachment {
  readonly id: string;
  /** Where the fitted image is on the phone: what the thumbnail shows. */
  readonly uri: string;
  readonly name: string;
  readonly mime: 'image/jpeg';
  /** Base64, as the daemon takes it. */
  readonly data: string;
}

export interface Picked {
  readonly uri: string;
  readonly width: number;
  readonly height: number;
  readonly name: string;
}

/**
 * A request may be 1 MiB, its words and its name included. Base64 of all the images of a
 * message may take this much of it.
 */
export const IMAGE_BUDGET = 700 * 1024;
export const MAX_IMAGES = 4;
/** Below this an image is no longer worth sending. */
export const LEAST_ROOM = 48 * 1024;

/** The longest side and the JPEG quality to try, in order, until the image fits. */
const LADDER: readonly { readonly side: number; readonly quality: number }[] = [
  { side: 1600, quality: 0.8 },
  { side: 1600, quality: 0.6 },
  { side: 1600, quality: 0.45 },
  { side: 1280, quality: 0.45 },
  { side: 1024, quality: 0.4 },
  { side: 800, quality: 0.35 },
  { side: 640, quality: 0.3 },
];

export type PickOutcome = { readonly picked: Picked } | { readonly picked: null; readonly why: 'cancelled' | 'refused' };

/** The system's own picker, or its camera. It asks for the camera the first time. */
export async function pick(source: 'library' | 'camera'): Promise<PickOutcome> {
  if (source === 'camera') {
    const allowed = await ImagePicker.requestCameraPermissionsAsync();
    if (!allowed.granted) return { picked: null, why: 'refused' };
  }
  const options: ImagePicker.ImagePickerOptions = { mediaTypes: ['images'], allowsMultipleSelection: false, quality: 1, exif: false };
  const result = source === 'camera' ? await ImagePicker.launchCameraAsync(options) : await ImagePicker.launchImageLibraryAsync(options);
  const asset = result.canceled ? undefined : result.assets[0];
  if (!asset) return { picked: null, why: 'cancelled' };
  return { picked: { uri: asset.uri, width: asset.width, height: asset.height, name: asset.fileName || (source === 'camera' ? 'photo.jpg' : 'image.jpg') } };
}

/**
 * The image as a JPEG whose base64 is at most `budget` characters: at most 1600 points on its
 * long side, then lower quality, then smaller, until it fits. `null` when nothing fits.
 */
export async function fit(picked: Picked, budget: number, id: string): Promise<Attachment | null> {
  let { width, height } = picked;
  for (const step of LADDER) {
    const context = ImageManipulator.manipulate(picked.uri);
    const long = Math.max(width, height);
    // The picker does not always know the size: then the image is asked, once it is read.
    if (long === 0 || long > step.side) context.resize(width >= height ? { width: step.side } : { height: step.side });
    const image = await context.renderAsync();
    if (long === 0) ({ width, height } = image);
    const saved = await image.saveAsync({ base64: true, compress: step.quality, format: SaveFormat.JPEG });
    if (saved.base64 && saved.base64.length <= budget) {
      return { id, uri: saved.uri, name: picked.name.replace(/\.[A-Za-z0-9]+$/, '') + '.jpg', mime: 'image/jpeg', data: saved.base64 };
    }
  }
  return null;
}

/** The images of a message as `run.follow_up` takes them. */
export function asImages(attachments: readonly Attachment[]): { mime: string; data: string; name: string }[] {
  return attachments.map((a) => ({ mime: a.mime, data: a.data, name: a.name }));
}
