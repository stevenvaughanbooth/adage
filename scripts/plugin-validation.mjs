// Parse the two observed Claude validator report markers without accepting missing records.
export function validatedHooks(report) {
  const records = [...report.matchAll(/^[ \t]*[>❯] \.\/register\.ts hooks: (.+)$/gm)]
  if (records.length !== 1) return null
  const text = records[0][1]
  const truncated = text.match(/… \[\+([1-9][0-9]*) chars\]$/)
  if (!truncated) return { prefix: text, omitted: 0 }
  const omitted = Number(truncated[1])
  return Number.isSafeInteger(omitted) ? { prefix: text.slice(0, truncated.index), omitted } : null
}

export function matchesValidatedHooks(listed, validated) {
  return validated !== null && listed.startsWith(validated.prefix)
    && listed.length === validated.prefix.length + validated.omitted
}
