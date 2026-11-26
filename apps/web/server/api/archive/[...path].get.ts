import { archivePathOf, readArchive } from '../../utils/archive-proxy'

/**
 * Same-origin boundary for the configured hosted archive. The archive's base URL stays in server
 * configuration; the browser and SSR both read through this route, so their answers agree.
 */
export default defineEventHandler(async (event): Promise<unknown> => {
  const result = await readArchive(
    () => useRuntimeConfig(event).archiveApiUrl,
    // A refused path is empty, which names no archive route.
    archivePathOf(getRouterParam(event, 'path') ?? '', event.node.req.url ?? '') ?? '',
    getQuery(event),
  )
  setResponseStatus(event, result.status)
  return result.body
})
