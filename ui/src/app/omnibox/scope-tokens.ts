/**
 * Scope tokens: the words that tell the omnibox *where* to search before the
 * query itself starts.
 *
 * `sett` at the front of the query suggests the Settings scope; `Tab` accepts
 * the suggestion and locks the search to it, the way a chat input's `#` locks
 * a channel. A locked scope is exclusive — filament profiles, printers and
 * slicing presets do not appear behind a Settings lock — because the user said
 * where to look, and second-guessing them is what made the palette feel like
 * everything at once.
 *
 * Everything here is pure matching; the scopes themselves live in the
 * registry ({@link ./scope-registry}) and the palette owns the interaction.
 */

/** One searchable corner of the app, named so a query can ask for it. */
export interface ScopeDef {
  /** Stable id the registry, the locked chips and the tests all speak. */
  readonly id: string;
  /** The name a chip and a suggestion row show. */
  readonly label: string;
  /** Iconoir name, from the same set every other surface uses. */
  readonly icon: string;
  /**
   * Words that engage the scope, lower-case. The first word of a query is
   * matched against these — an exact word engages by name, a prefix suggests
   * and `Tab` finishes the job.
   */
  readonly tokens: readonly string[];
  /** One line on what the scope holds — a suggestion's second line. */
  readonly hint: string;
}

/** What the first word of a query says about where to search. */
export interface ScopeParse {
  /** The scope the token names, when it names one exactly or by prefix. */
  readonly scope: ScopeDef | null;
  /** The query with the scope token removed, ready to search with. */
  readonly rest: string;
  /** The word only *prefixed* a token — `Tab` would finish the scope's name. */
  readonly partial: boolean;
}

/**
 * The scope `prefix` names, if any: an exact token wins outright, otherwise
 * the first registered scope with a token that extends the prefix. Registry
 * order is what keeps `f` meaning Filaments before Models' `files` — the
 * order scopes are listed is the order they are suggested, deliberately.
 */
export function matchScopePrefix(prefix: string, scopes: readonly ScopeDef[]): ScopeDef | null {
  const word = prefix.trim().toLowerCase();
  if (!word) {
    return null;
  }
  for (const scope of scopes) {
    if (scope.tokens.includes(word)) {
      return scope;
    }
  }
  for (const scope of scopes) {
    if (scope.tokens.some((token) => token.startsWith(word))) {
      return scope;
    }
  }
  return null;
}

/**
 * Split a query into its scope part and its search part.
 *
 * Only the **first** word can be a scope token — a scope is a place, named
 * once, not a filter sprinkled through the query. The rest keeps its inner
 * spacing: `settings  bed temp` searches for "bed temp".
 */
export function parseScopeQuery(query: string, scopes: readonly ScopeDef[]): ScopeParse {
  const trimmed = query.trimStart();
  const match = /^\S+/.exec(trimmed);
  if (!match) {
    return { scope: null, rest: query, partial: false };
  }
  const word = match[0];
  const scope = matchScopePrefix(word, scopes);
  if (!scope) {
    return { scope: null, rest: query, partial: false };
  }
  return {
    scope,
    rest: trimmed.slice(word.length).trimStart(),
    partial: !scope.tokens.includes(word.toLowerCase()),
  };
}

/**
 * Accept a suggestion: the scope joins the locked set, once.
 *
 * Locked scopes are a *union* — two chips search both corners — while any one
 * lock alone is exclusive against everything outside it.
 */
export function lockSuggestedScope(
  parse: ScopeParse,
  locked: readonly string[],
): readonly string[] {
  if (!parse.scope || locked.includes(parse.scope.id)) {
    return locked;
  }
  return [...locked, parse.scope.id];
}

/** Put the last-locked scope back, as `Backspace` on an empty query does. */
export function popLockedScope(locked: readonly string[]): readonly string[] {
  return locked.slice(0, -1);
}

/**
 * The scope a suggestion row should offer right now, if any: what the query
 * names, unless it is already locked — offering what is already on would make
 * `Tab` a no-op that looks like an action.
 */
export function suggestedScope(parse: ScopeParse, locked: readonly string[]): ScopeDef | null {
  if (!parse.scope || locked.includes(parse.scope.id)) {
    return null;
  }
  return parse.scope;
}
