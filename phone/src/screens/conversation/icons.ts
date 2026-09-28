import { ICONS, type IconName } from '@/ui/icons.generated';

/** The model names icons as VS Code does (codicons). One the font does not have becomes `fallback`. */
export function iconOf(name: string | null | undefined, fallback: IconName = 'circle-small'): IconName {
  return name && Object.hasOwn(ICONS, name) ? (name as IconName) : fallback;
}
