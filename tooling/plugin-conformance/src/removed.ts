/**
 * Stand-ins for functions a suite still names but upstream removed outright
 * (not moved). A case that calls one records `threw` with the commit that
 * removed it, so the case stays in its vector file until the re-port decides
 * what replaces it.
 */
export const removedUpstream =
  (name: string, commit: string) =>
  (..._args: unknown[]): never => {
    throw new Error(`${name} was removed upstream in ${commit}`);
  };
