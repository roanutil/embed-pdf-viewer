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
import type { attachmentPlugin as AttachmentPlugin } from '@embedpdf/plugin-attachment';
import type { commandsPlugin as CommandsPlugin } from '@embedpdf/plugin-commands';
import type { formPlugin as FormPlugin } from '@embedpdf/plugin-form';
import type { historyPlugin as HistoryPlugin } from '@embedpdf/plugin-history';
import type { i18nPlugin as I18nPlugin } from '@embedpdf/plugin-i18n';
import type {
  feedbackPlugin as FeedbackPlugin,
  interactionPlugin as InteractionPlugin,
} from '@embedpdf/plugin-interaction';
import type { linkPlugin as LinkPlugin } from '@embedpdf/plugin-link';
import type { measurementPlugin as MeasurementPlugin } from '@embedpdf/plugin-measurement';
import type { metadataPlugin as MetadataPlugin } from '@embedpdf/plugin-metadata';
import type { outlinePlugin as OutlinePlugin } from '@embedpdf/plugin-outline';
import type { pageEditPlugin as PageEditPlugin } from '@embedpdf/plugin-page-edit';
import type { redactionPlugin as RedactionPlugin } from '@embedpdf/plugin-redaction';
import type { renderPlugin as RenderPlugin } from '@embedpdf/plugin-render';
import type { searchPlugin as SearchPlugin } from '@embedpdf/plugin-search';
import type { selectionPlugin as SelectionPlugin } from '@embedpdf/plugin-selection';
import type { shellPlugin as ShellPlugin } from '@embedpdf/plugin-shell';
import type { signaturePlugin as SignaturePlugin } from '@embedpdf/plugin-signature';
import type { Scheduler, stagePlugin as StagePlugin } from '@embedpdf/plugin-stage';
import type { stampPlugin as StampPlugin } from '@embedpdf/plugin-stamp';
import type { viewManagerPlugin as ViewManagerPlugin } from '@embedpdf/plugin-view-manager';
import { createSandboxBridge } from './sandbox';

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
 * The browser globals the TypeScript plugins reach for, which the Rust ones
 * take as host inputs through their adapters' live config: stage's frame
 * `scheduler`, render's `requestFrame` and console lines, and annotation's
 * `randomUuid` and `consoleError`. Set once, before the factories run; each
 * factory adds its part to the config it returns. Unset, stage is
 * frameless, render schedules on the kernel's clock, and annotation ids
 * count up from the same first id every session.
 */
export interface Hosts {
  scheduler?: Scheduler;
  render?: {
    requestFrame?: (frame: () => void) => void;
    warn?: (message: string) => void;
    debug?: (message: string) => void;
  };
  annotation?: {
    randomUuid?: () => string;
    consoleError?: (message: string, error?: unknown) => void;
  };
}

let hosts: Hosts = {};

export function setHosts(next: Hosts): void {
  hosts = next;
}

/**
 * `id` and `token` name one lens: the stage adapter takes a single `lens`
 * name for both (`crates/plugins/stage/src/harness.rs`, `stage_options`), so
 * a lens whose id and token name differ is refused. `scheduler`, or the
 * hosts' one, is the adapter's live `scheduler: {raf, caf}`.
 */
export const stagePlugin: typeof StagePlugin = (options = {}) => {
  const { id = 'stage', token, scheduler = hosts.scheduler, ...settings } = options;
  const tokenName = token?.name ?? 'stage';
  if (id !== tokenName) {
    throw new Error(
      `[core-wasm] stagePlugin: id "${id}" and token "${tokenName}" differ; the Rust stage names both from one lens`,
    );
  }
  const frames = scheduler
    ? { raf: scheduler.raf.bind(scheduler), caf: scheduler.caf.bind(scheduler) }
    : undefined;
  return rust<Def<typeof StagePlugin>>('stage', {
    ...settings,
    ...(id === 'stage' ? {} : { lens: id }),
    ...(frames ? { scheduler: frames } : {}),
  });
};

export const renderPlugin: typeof RenderPlugin = (config = {}) =>
  rust('render', hosts.render ? { ...config, host: hosts.render } : config);
export const pageEditPlugin: typeof PageEditPlugin = () => rust('page-edit');
export const interactionPlugin: typeof InteractionPlugin = (config = {}) =>
  rust('interaction', config);

/** The adapter takes `provider: true` for its logging provider, which a
 *  live kernel does not log: haptics are silent in the eval build. */
export const feedbackPlugin: typeof FeedbackPlugin = (options = {}) =>
  rust('feedback', { provider: options.provider !== undefined });

export const selectionPlugin: typeof SelectionPlugin = (config = {}) => rust('selection', config);
export const annotationPlugin: typeof AnnotationPlugin = (config = {}) =>
  rust('annotation', hosts.annotation ? { ...config, host: hosts.annotation } : config);

/** `assetEngine` is not carried: the adapter only knows its scripted ones. */
export const stampPlugin: typeof StampPlugin = (config = {}) =>
  rust('stamp', { previewWidth: config.previewWidth, dynamic: config.dynamic });

/**
 * With scripting on, the sandbox is `sandboxFactory` or, as in the
 * TypeScript, the QuickJS one, handed to the adapter as the live `sandbox`
 * it calls back (`./sandbox`).
 */
export const actionsPlugin: typeof ActionsPlugin = (config) => {
  const javascript = config?.javascript;
  if (!javascript?.enabled) return rust('actions', config);
  const { sandboxFactory, ...settings } = javascript;
  return rust('actions', {
    ...config,
    javascript: { ...settings, sandbox: createSandboxBridge(sandboxFactory) },
  });
};
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
export const historyPlugin: typeof HistoryPlugin = (config) => rust('history', config);
export const outlinePlugin: typeof OutlinePlugin = () => rust('outline');
export const attachmentPlugin: typeof AttachmentPlugin = () => rust('attachment');
