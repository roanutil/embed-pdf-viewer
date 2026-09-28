/**
 * One factory per plugin the chrome registers, with the TypeScript name and
 * config type. Each answers a descriptor, `{ $rust: <factory>, config }`,
 * that `createKernel` hands to the Rust plugin's conformance adapter
 * (`crates/plugins/<id>/src/harness.rs`, `build`), so `config` is the JSON
 * that adapter reads. Functions in it become `{"$fn": "js:<id>"}` when the
 * kernel is built.
 */
import type { AnyPlugin } from '@embedpdf/core';
import type { actionsPlugin as ActionsPlugin } from '@embedpdf/plugin-actions';
import type { annotationPlugin as AnnotationPlugin } from '@embedpdf/plugin-annotation';
import type { commandsPlugin as CommandsPlugin } from '@embedpdf/plugin-commands';
import type { formPlugin as FormPlugin } from '@embedpdf/plugin-form';
import type { i18nPlugin as I18nPlugin } from '@embedpdf/plugin-i18n';
import type {
  feedbackPlugin as FeedbackPlugin,
  interactionPlugin as InteractionPlugin,
} from '@embedpdf/plugin-interaction';
import type { linkPlugin as LinkPlugin } from '@embedpdf/plugin-link';
import type { measurementPlugin as MeasurementPlugin } from '@embedpdf/plugin-measurement';
import type { metadataPlugin as MetadataPlugin } from '@embedpdf/plugin-metadata';
import type { pageEditPlugin as PageEditPlugin } from '@embedpdf/plugin-page-edit';
import type { redactionPlugin as RedactionPlugin } from '@embedpdf/plugin-redaction';
import type { renderPlugin as RenderPlugin } from '@embedpdf/plugin-render';
import type { searchPlugin as SearchPlugin } from '@embedpdf/plugin-search';
import type { selectionPlugin as SelectionPlugin } from '@embedpdf/plugin-selection';
import type { shellPlugin as ShellPlugin } from '@embedpdf/plugin-shell';
import type { signaturePlugin as SignaturePlugin } from '@embedpdf/plugin-signature';
import type { stagePlugin as StagePlugin } from '@embedpdf/plugin-stage';
import type { stampPlugin as StampPlugin } from '@embedpdf/plugin-stamp';
import type { viewManagerPlugin as ViewManagerPlugin } from '@embedpdf/plugin-view-manager';

/** A Rust plugin, as the kernel's plugin list carries it. */
export interface RustPluginDef {
  readonly id: string;
  readonly $rust: string;
  readonly config?: unknown;
}

export const isRustPlugin = (plugin: AnyPlugin | RustPluginDef): plugin is RustPluginDef =>
  typeof (plugin as RustPluginDef).$rust === 'string';

/** A descriptor, typed as the TypeScript factory's `PluginDef`. */
const rust = <T>(factory: string, config?: unknown): T =>
  ({ id: factory, $rust: factory, config }) as RustPluginDef as T;

type Def<F extends (...args: never[]) => unknown> = ReturnType<F>;

/**
 * `id` and `token` name one lens: the stage adapter takes a single `lens`
 * name for both (`crates/plugins/stage/src/harness.rs`, `stage_options`), so
 * a lens whose id and token name differ is refused. `scheduler` is dropped:
 * the Rust stage schedules on the kernel's clock.
 */
export const stagePlugin: typeof StagePlugin = (options = {}) => {
  const { id = 'stage', token, scheduler: _scheduler, ...settings } = options;
  const tokenName = token?.name ?? 'stage';
  if (id !== tokenName) {
    throw new Error(
      `[core-wasm] stagePlugin: id "${id}" and token "${tokenName}" differ; the Rust stage names both from one lens`,
    );
  }
  return rust<Def<typeof StagePlugin>>(
    'stage',
    id === 'stage' ? settings : { ...settings, lens: id },
  );
};

export const renderPlugin: typeof RenderPlugin = (config = {}) => rust('render', config);
export const pageEditPlugin: typeof PageEditPlugin = () => rust('page-edit');
export const interactionPlugin: typeof InteractionPlugin = (config = {}) =>
  rust('interaction', config);

/** The adapter takes `provider: true` for its logging provider, which a
 *  live kernel does not log: haptics are silent in the eval build. */
export const feedbackPlugin: typeof FeedbackPlugin = (options = {}) =>
  rust('feedback', { provider: options.provider !== undefined });

export const selectionPlugin: typeof SelectionPlugin = (config = {}) => rust('selection', config);
export const annotationPlugin: typeof AnnotationPlugin = (config = {}) =>
  rust('annotation', config);

/** `assetEngine` is not carried: the adapter only knows its scripted ones. */
export const stampPlugin: typeof StampPlugin = (config = {}) =>
  rust('stamp', { previewWidth: config.previewWidth, dynamic: config.dynamic });

export const actionsPlugin: typeof ActionsPlugin = (config) => rust('actions', config);
/** The adapter builds the form plugin from no config. */
export const formPlugin: typeof FormPlugin = (config = {}) => rust('form', config);
export const signaturePlugin: typeof SignaturePlugin = (config = {}) => rust('signature', config);
export const linkPlugin: typeof LinkPlugin = () => rust('link');
export const redactionPlugin: typeof RedactionPlugin = (config = {}) => rust('redaction', config);
export const measurementPlugin: typeof MeasurementPlugin = (config = {}) =>
  rust('measurement', config);
export const searchPlugin: typeof SearchPlugin = (config) => rust('search', config);
export const i18nPlugin: typeof I18nPlugin = (config = {}) => rust('i18n', config);
export const commandsPlugin: typeof CommandsPlugin = (config = {}) => rust('commands', config);
export const shellPlugin: typeof ShellPlugin = () => rust('shell');
export const metadataPlugin: typeof MetadataPlugin = () => rust('metadata');
export const viewManagerPlugin: typeof ViewManagerPlugin = () => rust('view-manager');
