import { memo, useCallback, useMemo, useState } from 'react';
import { Image, TextInput, useWindowDimensions, View } from 'react-native';

import { text, type agents } from '@/model';
import { Tap } from '@/motion';
import { useCapabilities } from '@/platform';
import { lineHeight, useTheme } from '@/theme';
import { Chip, Icon, makeStyles, MAX_TEXT_SCALE, Menu, Txt, type MenuItem } from '@/ui';

import { asImages, fit, IMAGE_BUDGET, LEAST_ROOM, MAX_IMAGES, pick, type Attachment } from './attach';
import type { FollowUp } from './held';
import type { TurnChoices } from './options';
import { useDraft } from './useDraft';
import { WORDS } from './words';

/** A request may be 1 MiB; this much is left for its words beside the images. */
const LONGEST = 1024 * 1024 - 16 * 1024;

const useStyles = makeStyles((theme) => {
  const line = lineHeight(theme.font.xl, theme.line.body);
  return {
    composer: { paddingHorizontal: theme.space[3], paddingTop: theme.space[2], paddingBottom: theme.space[2], gap: theme.space[2] },
    text: {
      minHeight: theme.phone.size.touch,
      paddingHorizontal: theme.space[3],
      paddingTop: theme.space[3],
      paddingBottom: theme.space[3],
      borderRadius: theme.radius.card,
      borderWidth: theme.phone.size.hairline,
      borderColor: theme.colors.border,
      backgroundColor: theme.colors.raised,
      color: theme.colors.text,
      fontSize: theme.font.xl,
      lineHeight: line,
    },
    unused: { opacity: theme.phone.opacity.disabled },
    row: { flexDirection: 'row', alignItems: 'flex-end', gap: theme.space[2] },
    chips: { flex: 1, flexDirection: 'row', flexWrap: 'wrap', alignItems: 'center', gap: theme.space[2] },
    round: { width: theme.phone.size.touch, height: theme.phone.size.touch, borderRadius: theme.radius.pill, alignItems: 'center', justifyContent: 'center' },
    send: { backgroundColor: theme.colors.accentStrong },
    stop: { backgroundColor: theme.colors.raised2, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.borderStrong },
    images: { flexDirection: 'row', flexWrap: 'wrap', gap: theme.space[2] },
    image: { flexDirection: 'row', alignItems: 'center', borderRadius: theme.radius.control, borderWidth: theme.phone.size.hairline, borderColor: theme.colors.border, backgroundColor: theme.colors.raised, overflow: 'hidden' },
    thumb: { width: theme.phone.size.touch, height: theme.phone.size.touch },
    remove: { width: theme.phone.size.touch, height: theme.phone.size.touch, alignItems: 'center', justifyContent: 'center' },
  };
});

export interface ComposerProps {
  readonly runId: string;
  readonly header: agents.RunHeader;
  /** The model the agent runs with now, as the Mac says it. */
  readonly model: string | null;
  readonly choices: TurnChoices;
  /** True while the agent works: what is sent waits for the turn to end. */
  readonly busy: boolean;
  readonly onSend: (message: Omit<FollowUp, 'run_id'>) => void;
  readonly onStop: () => void;
}

type Open = 'none' | 'model' | 'effort' | 'mode' | 'attach';

/**
 * Where a message to the agent is written: several lines, the choices for this message under
 * them, an image, Send. While the agent works, Send queues the message and Stop stops the turn.
 */
export const Composer = memo(function Composer({ runId, header, model, choices, busy, onSend, onStop }: ComposerProps) {
  const styles = useStyles();
  const theme = useTheme();
  const { fontScale } = useWindowDimensions();
  const { camera, random } = useCapabilities();
  // Five lines of the text as the system draws it, then it scrolls.
  const tallest = useMemo(
    () => ({ maxHeight: Math.ceil(theme.phone.size.composerMaxLines * lineHeight(theme.font.xl, theme.line.body) * Math.min(Math.max(fontScale, 1), MAX_TEXT_SCALE)) + theme.space[3] * 2 }),
    [theme, fontScale],
  );
  const [draft, setDraft] = useDraft(runId);
  const [images, setImages] = useState<readonly Attachment[]>([]);
  const [chosen, setChosen] = useState({ model: '', effort: '', mode: '' });
  const [open, setOpen] = useState<Open>('none');
  const [notice, setNotice] = useState('');
  const [hasCamera, setHasCamera] = useState(false);
  const close = useCallback(() => setOpen('none'), []);

  const canSend = header.canSend;
  const ready = canSend && draft.trim().length > 0;

  const send = useCallback(() => {
    const prompt = draft.trim();
    if (!canSend || !prompt) return;
    const sent = asImages(images);
    if (prompt.length + sent.reduce((n, image) => n + image.data.length, 0) > LONGEST) {
      setNotice(WORDS.tooLong);
      return;
    }
    onSend({
      prompt,
      ...(chosen.model ? { model: chosen.model } : {}),
      ...(chosen.effort ? { effort: chosen.effort } : {}),
      ...(chosen.mode ? { permission_mode: chosen.mode } : {}),
      ...(sent.length > 0 ? { images: sent } : {}),
    });
    setDraft('');
    setImages([]);
    setNotice('');
  }, [draft, canSend, images, chosen, onSend, setDraft]);

  const attach = useCallback(
    async (source: 'library' | 'camera') => {
      setNotice('');
      try {
        const outcome = await pick(source);
        if (outcome.picked === null) {
          if (outcome.why === 'refused') setNotice(WORDS.cameraRefused);
          return;
        }
        const used = images.reduce((n, image) => n + image.data.length, 0);
        const fitted = await fit(outcome.picked, IMAGE_BUDGET - used, random.uuid());
        if (fitted === null) setNotice(WORDS.imageTooLarge);
        else setImages((before) => [...before, fitted]);
      } catch {
        setNotice(WORDS.imageFailed);
      }
    },
    [images, random],
  );

  const openAttach = useCallback(() => {
    const used = images.reduce((n, image) => n + image.data.length, 0);
    if (images.length >= MAX_IMAGES || IMAGE_BUDGET - used < LEAST_ROOM) {
      setNotice(WORDS.imageNoRoom);
      return;
    }
    // Asked when it matters: a simulator has no camera, and says so without a prompt.
    camera.support().then(
      (support) => setHasCamera(support.supported),
      () => setHasCamera(false),
    );
    setOpen('attach');
  }, [images, camera]);

  const menus = useMemo(() => {
    const choice = (kind: 'model' | 'effort' | 'mode', value: string, label: string): MenuItem => ({
      id: value || 'default',
      label,
      icon: chosen[kind] === value ? 'check' : 'blank',
      onPress: () => setChosen((before) => ({ ...before, [kind]: value })),
    });
    return {
      model: [choice('model', '', WORDS.standard), ...choices.models.map((m) => choice('model', m, m))],
      effort: [choice('effort', '', WORDS.standard), ...choices.efforts.map((e) => choice('effort', e, e))],
      mode: [choice('mode', '', WORDS.standard), ...choices.modes.map((m) => choice('mode', m.value, m.label))],
    };
  }, [choices, chosen]);

  const attachItems = useMemo<readonly MenuItem[]>(
    () => [
      { id: 'library', label: WORDS.chooseImage, icon: 'file-media', onPress: () => void attach('library') },
      ...(hasCamera ? [{ id: 'camera', label: WORDS.takePhoto, detail: WORDS.takePhotoDetail, icon: 'device-camera', onPress: () => void attach('camera') } satisfies MenuItem] : []),
    ],
    [attach, hasCamera],
  );

  const modelNow = chosen.model || model || '';
  const modeNow = choices.modes.find((m) => m.value === chosen.mode);

  return (
    <View testID="agent.composer" style={styles.composer}>
      {images.length > 0 ? (
        <View style={styles.images}>
          {images.map((image, index) => (
            <View key={image.id} testID={`agent.composer.image.${index}`} style={styles.image}>
              <Image source={{ uri: image.uri }} accessibilityLabel={image.name} style={styles.thumb} />
              <Tap testID={`agent.composer.image.${index}.remove`} accessibilityLabel={WORDS.removeImage(image.name)} haptic="selection" onPress={() => setImages((before) => before.filter((other) => other.id !== image.id))} style={styles.remove}>
                <Icon name="close" size="md" tone="muted" />
              </Tap>
            </View>
          ))}
        </View>
      ) : null}
      {notice ? (
        <Txt testID="agent.composer.notice" kind="small" tone="amber" accessibilityLiveRegion="polite">
          {notice}
        </Txt>
      ) : null}
      <TextInput
        testID="agent.composer.text"
        accessibilityLabel={WORDS.message}
        value={draft}
        onChangeText={setDraft}
        editable={canSend}
        multiline
        placeholder={header.placeholder}
        placeholderTextColor={theme.colors.muted}
        selectionColor={theme.colors.accent}
        maxFontSizeMultiplier={MAX_TEXT_SCALE}
        textAlignVertical="top"
        style={[styles.text, tallest, canSend ? null : styles.unused]}
      />
      <View style={styles.row}>
        <View style={styles.chips}>
          {canSend && choices.models.length > 0 ? <Chip testID="agent.composer.model" label={modelNow || WORDS.model} accessibilityLabel={`${WORDS.model}: ${modelNow || WORDS.standard}`} icon="sparkle" selected={chosen.model !== ''} haptic="selection" onPress={() => setOpen('model')} /> : null}
          {canSend && choices.efforts.length > 0 ? <Chip testID="agent.composer.effort" label={chosen.effort ? WORDS.effortOf(chosen.effort) : WORDS.effort} accessibilityLabel={`${WORDS.effort}: ${chosen.effort || WORDS.standard}`} icon="dashboard" selected={chosen.effort !== ''} haptic="selection" onPress={() => setOpen('effort')} /> : null}
          {canSend && choices.modes.length > 0 ? <Chip testID="agent.composer.mode" label={modeNow?.label ?? WORDS.permissions} accessibilityLabel={`${WORDS.permissions}: ${modeNow?.label ?? WORDS.standard}`} icon={modeNow?.icon ?? 'shield'} selected={chosen.mode !== ''} haptic="selection" onPress={() => setOpen('mode')} /> : null}
        </View>
        {canSend && choices.images ? (
          <Tap testID="agent.composer.attach" accessibilityLabel={WORDS.attach} haptic="selection" onPress={openAttach} style={styles.round}>
            <Icon name="attach" size="lg" tone="muted" />
          </Tap>
        ) : null}
        {header.canStop ? (
          <Tap testID="agent.composer.stop" accessibilityLabel={text.TEXT.chat.stop} haptic="impact" onPress={onStop} style={[styles.round, styles.stop]}>
            <Icon name="debug-stop" size="lg" tone="red" />
          </Tap>
        ) : null}
        <Tap testID="agent.composer.send" accessibilityLabel={busy ? text.TEXT.chat.queueMessage : text.TEXT.chat.send} disabled={!ready} haptic="confirm" onPress={send} style={[styles.round, styles.send]}>
          <Icon name={busy ? 'history' : 'arrow-up'} size="lg" tone="onAccent" />
        </Tap>
      </View>
      <Menu testID="agent.composer.model" open={open === 'model'} onClose={close} title={WORDS.model} items={menus.model} />
      <Menu testID="agent.composer.effort" open={open === 'effort'} onClose={close} title={WORDS.effort} items={menus.effort} />
      <Menu testID="agent.composer.mode" open={open === 'mode'} onClose={close} title={WORDS.permissions} items={menus.mode} />
      <Menu testID="agent.composer.attach" open={open === 'attach'} onClose={close} title={WORDS.attach} items={attachItems} />
    </View>
  );
});
