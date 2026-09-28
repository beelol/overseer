import { Camera, CameraView, type PermissionResponse } from 'expo-camera';

import { SUPPORTED, defineCapability, unsupported } from '../capability';
import type { CameraCapability, CameraPermission, CodeScannerProps } from '../capabilities/camera';
import type { LaunchCapability } from '../capabilities/launch';

function toPermission(response: PermissionResponse): CameraPermission {
  if (response.granted) return 'granted';
  return response.canAskAgain && response.status === 'undetermined' ? 'undetermined' : 'denied';
}

const onlyQrCodes = { barcodeTypes: ['qr' as const] };

function CodeScanner({ active, onCode, accessibilityLabel, style }: CodeScannerProps) {
  return (
    <CameraView
      accessible
      accessibilityLabel={accessibilityLabel}
      active={active}
      barcodeScannerSettings={onlyQrCodes}
      facing="back"
      onBarcodeScanned={active ? (result) => onCode(result.data) : undefined}
      style={style}
    />
  );
}

/** The camera on both platforms, through expo-camera. */
export function createCamera(launch: LaunchCapability): CameraCapability {
  return defineCapability<CameraCapability>(
    'camera',
    async () =>
      launch.info().isSimulator
        ? unsupported('Simulators have no camera. Type the pairing code instead.')
        : SUPPORTED,
    {
      permission: async () => toPermission(await Camera.getCameraPermissionsAsync()),
      requestPermission: async () => toPermission(await Camera.requestCameraPermissionsAsync()),
      CodeScanner,
    },
  );
}
