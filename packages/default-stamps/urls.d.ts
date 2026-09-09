/** The shipped locale codes, in the package's order. */
export declare const LOCALES: readonly string[];
/** `<locale>/stamps.pdf` per shipped locale, resolved by the consumer's
 *  bundler (or at runtime for native ESM). See urls.js for the contract. */
export declare const urls: Readonly<Record<string, string>>;
/** jsDelivr template with a `{locale}` slot, pinned to this major. */
export declare const CDN_URL_TEMPLATE: string;
