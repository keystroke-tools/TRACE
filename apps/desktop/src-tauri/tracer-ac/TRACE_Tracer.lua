local bridgeUrl = 'http://127.0.0.1:18081'
local bridgeHeaders = {
  ['Content-Type'] = 'application/json',
  ['X-Trace-Tracer'] = '1'
}
local profile = nil
local profileError = nil
local sessions = nil
local requestState = 'idle'
local requestMessage = nil
local profileLoaded = false
local sessionsRequested = false
local includeOtherTracks = false
local expandedSessionId = nil
local pendingSelection = nil
local profileTrackOverride = false
local settingsPage = 'reference'
local configPath = ac.getFolder(ac.FolderID.ScriptConfig) .. '/reference.json'
local colors = {
  text = rgbm(0.94, 0.94, 0.94, 1),
  muted = rgbm(0.52, 0.55, 0.56, 1),
  red = rgbm(0.92, 0.25, 0.25, 1),
  purple = rgbm(0.70, 0.42, 1.00, 1),
  surface = rgbm(0.08, 0.08, 0.08, 0.96),
  raised = rgbm(0.12, 0.12, 0.12, 1),
  track = rgbm(0.18, 0.18, 0.18, 1)
}
local hudPadding = 12
local throttleCuePercent = 30
local brakeTraceBehindMetres = 220
local brakeTraceAheadMetres = 80
local liveBrakeTrace = {
  samples = {},
  lastDistanceM = nil,
  identity = nil
}

local function currentIdentity()
  return {
    trackId = ac.getTrackID(),
    layoutId = ac.getTrackLayout() ~= '' and ac.getTrackLayout() or nil,
    carId = ac.getCarID(0)
  }
end

local function parseResponse(err, response)
  if err and err ~= '' then return nil, err end
  if not response then return nil, 'TRACE did not return a response.' end
  local ok, value = pcall(JSON.parse, response.body or '')
  if not ok or type(value) ~= 'table' then
    return nil, 'TRACE returned an invalid response.'
  end
  if response.status < 200 or response.status >= 300 then
    return nil, value.error or string.format('TRACE returned HTTP %d.', response.status)
  end
  return value, nil
end

local function ensureThrottleCues(reference)
  if type(reference.throttleCues) == 'table' and #reference.throttleCues > 0 then return end
  reference.throttleCues = {}
  local samples = reference.samples or {}
  local brakeZones = reference.brakeZones or {}
  for zoneIndex = 1, #brakeZones do
    local zone = brakeZones[zoneIndex]
    local nextZone = brakeZones[zoneIndex + 1]
    for sampleIndex = 1, #samples do
      local sample = samples[sampleIndex]
      if sample.d >= zone.endM
          and (not nextZone or sample.d < nextZone.startM)
          and (sample.t or 0) >= throttleCuePercent then
        table.insert(reference.throttleCues, { startM = sample.d })
        break
      end
    end
  end
end

local function loadProfile()
  local encoded = io.load(configPath, '')
  if encoded == '' then
    profile = nil
    profileError = 'Choose a matching session first.'
    return false
  end
  local ok, value = pcall(JSON.parse, encoded)
  if not ok or type(value) ~= 'table' or value.schemaVersion ~= 1 then
    profile = nil
    profileError = 'The generated reference is invalid or unsupported.'
    return false
  end
  ensureThrottleCues(value)
  profile = value
  profileError = nil
  return true
end

local function refreshSessions()
  local identity = currentIdentity()
  if not identity.carId or identity.trackId == '' then
    requestState = 'error'
    requestMessage = 'Waiting for the current car and track.'
    return
  end
  requestState = 'loading'
  requestMessage = nil
  identity.includeOtherTracks = includeOtherTracks
  web.post(bridgeUrl .. '/api/tracer/sessions', bridgeHeaders, JSON.stringify(identity), function(err, response)
    local value, parseError = parseResponse(err, response)
    if not value then
      sessions = nil
      requestState = 'error'
      requestMessage = 'TRACE is unavailable. Keep the desktop app running.\n' .. (parseError or '')
      return
    end
    sessions = value
    requestState = 'ready'
    requestMessage = #sessions == 0 and (includeOtherTracks and 'No recorded laps match this car.' or 'No recorded laps match this car, track, and layout.') or nil
  end)
end

local function generateReference(sessionId, lapIndex, lapTime, allowTrackMismatch)
  local request = currentIdentity()
  request.sessionId = sessionId
  request.lapIndex = lapIndex
  request.allowTrackMismatch = allowTrackMismatch
  requestState = 'preparing'
  requestMessage = string.format('Rebuilding %s…', lapTime)
  web.post(bridgeUrl .. '/api/tracer/reference', bridgeHeaders, JSON.stringify(request), function(err, response)
    local value, parseError = parseResponse(err, response)
    if not value then
      requestState = 'error'
      requestMessage = parseError or 'TRACE could not prepare this reference.'
      return
    end
    if not loadProfile() then
      requestState = 'error'
      requestMessage = profileError
      return
    end
    local source = profile.source or {}
    if value.sessionId ~= sessionId or value.lapIndex ~= lapIndex or source.sessionId ~= sessionId or source.lapIndex ~= lapIndex then
      profile = nil
      profileError = 'TRACE returned a different lap than the one selected.'
      requestState = 'error'
      requestMessage = profileError
      return
    end
    requestState = 'ready'
    requestMessage = string.format('Loaded lap %d · %s', lapIndex, lapTime)
    profileTrackOverride = allowTrackMismatch
    pendingSelection = nil
  end)
end

local function activateSession(session, lap)
  generateReference(session.id, lap.index, lap.time, not session.exactMatch)
end

local function sampleAt(distanceM)
  local samples = profile.samples
  if not samples or #samples == 0 then return nil end
  local spacing = math.max(profile.sampleSpacingM or 5, 1)
  local index = math.clamp(math.floor(distanceM / spacing + 0.5) + 1, 1, #samples)
  return samples[index]
end

local function liveBrakePercent(value)
  if not value then return 0 end
  return math.clamp(value * 100, 0, 100)
end

local function recordLiveBrake(state)
  if not profile or not state or not state.car then return end
  local source = profile.source or {}
  local identity = string.format('%s:%s', source.sessionId or '', source.lapIndex or '')
  if liveBrakeTrace.identity ~= identity then
    liveBrakeTrace.samples = {}
    liveBrakeTrace.lastDistanceM = nil
    liveBrakeTrace.identity = identity
  end
  if liveBrakeTrace.lastDistanceM
      and state.distanceM < liveBrakeTrace.lastDistanceM - profile.trackLengthM * 0.5 then
    liveBrakeTrace.samples = {}
  end
  liveBrakeTrace.lastDistanceM = state.distanceM
  local spacing = math.max(profile.sampleSpacingM or 5, 1)
  local index = math.floor(state.distanceM / spacing + 0.5) + 1
  liveBrakeTrace.samples[index] = liveBrakePercent(state.car.brake)
end

local function liveBrakeAt(distanceM)
  if not profile then return nil end
  local spacing = math.max(profile.sampleSpacingM or 5, 1)
  local index = math.floor(distanceM / spacing + 0.5) + 1
  return liveBrakeTrace.samples[index]
end

local function referenceLapDuration()
  local samples = profile and profile.samples or nil
  if not samples then return nil end
  for index = #samples, 1, -1 do
    local elapsed = samples[index].e
    if elapsed then return elapsed end
  end
  return nil
end

local function activeOrNextZone(distanceM)
  local zones = profile.brakeZones or {}
  for i = 1, #zones do
    if zones[i].endM >= distanceM then return zones[i] end
  end
  return zones[1]
end

local function sessionTitle(session)
  local identity = session.driver or 'Unknown driver'
  if session.title and session.title ~= session.driver then
    identity = identity .. '  ·  ' .. session.title
  end
  return string.format('%s  ·  %s', identity, session.bestLapTime or 'no valid lap')
end

local function sessionDetail(session)
  local date = (session.startedAt or ''):gsub('T', ' '):sub(1, 16)
  local sessionType = string.upper(session.sessionType or 'session')
  local track = session.track or 'Unknown track'
  if session.layoutId and session.layoutId ~= '' then track = track .. ' / ' .. session.layoutId end
  return string.format('%s  ·  %s  ·  %s', sessionType, track, date)
end

local function lapLabel(session, lap)
  local status = lap.isFastest and 'FASTEST' or string.upper(lap.validity or 'recorded')
  return string.format('LAP %d    %s    %s###lap-%s-%d', lap.index, lap.time, status, session.id, lap.index)
end

local function drawSessionPicker()
  local source = profile and profile.source or nil
  if source then
    local identity = source.driver or 'Recorded session'
    if source.title and source.title ~= source.driver then identity = identity .. '  ·  ' .. source.title end
    local start = ui.getCursor()
    local width = ui.availableSpaceX()
    ui.drawRectFilled(start, start + vec2(width, 58), rgbm(0.18, 0.11, 0.25, 1), 4)
    ui.setCursor(start + vec2(10, 9))
    ui.dwriteText('ACTIVE REFERENCE', 10, colors.purple)
    ui.setCursor(start + vec2(10, 32))
    ui.dwriteText(string.format('%s  ·  LAP %d  ·  %s', identity, source.lapIndex, source.lapTime), 11, colors.text)
    ui.setCursor(start + vec2(width - 104, 8))
    if requestState ~= 'preparing' and ui.button('REBUILD', vec2(94, 28)) then
      local identity = currentIdentity()
      local allowMismatch = profileTrackOverride
          or identity.trackId ~= profile.trackId
          or (identity.layoutId or '') ~= (profile.layoutId or '')
      generateReference(source.sessionId, source.lapIndex, source.lapTime, allowMismatch)
    end
    ui.setCursor(start + vec2(0, 64))
  end
  ui.dwriteText('REFERENCE LAP', 13, colors.text)
  ui.dwriteText(string.format('%s  ·  %s', ac.getCarName(0) or ac.getCarID(0) or 'Unknown car', ac.getTrackName() or ac.getTrackID()), 11, colors.muted)
  ui.dummy(3)
  if requestState ~= 'loading' and requestState ~= 'preparing' and ui.button('REFRESH', vec2(76, 24)) then
    refreshSessions()
  end
  ui.sameLine()
  local modeLabel = includeOtherTracks and 'EXACT MATCHES' or 'OTHER TRACKS'
  if requestState ~= 'loading' and requestState ~= 'preparing' and ui.button(modeLabel, vec2(112, 24)) then
    includeOtherTracks = not includeOtherTracks
    expandedSessionId = nil
    pendingSelection = nil
    refreshSessions()
  end
  ui.dummy(6)

  if requestState == 'loading' then
    ui.dwriteText('Finding matching sessions in TRACE…', 13, colors.purple)
    return
  end
  if requestState == 'preparing' then
    ui.dwriteText(requestMessage or 'Preparing reference…', 16, colors.purple)
    ui.dwriteText('TRACE is processing the recorded telemetry. Please wait.', 11, colors.muted)
    return
  end
  if requestMessage then
    ui.dwriteText(requestMessage, 12, requestState == 'error' and colors.red or colors.muted)
  end
  if not sessions or #sessions == 0 then return end

  local selectedSession = nil
  for i = 1, #sessions do
    if sessions[i].id == expandedSessionId then selectedSession = sessions[i] end
  end

  local availableHeight = ui.availableSpaceY()
  local sessionListHeight = selectedSession and math.max(82, math.min(availableHeight * 0.42, 190)) or availableHeight
  ui.dwriteText('SESSIONS', 10, colors.muted)
  ui.childWindow('matchingSessions', vec2(0, sessionListHeight), false, function()
    for i = 1, #sessions do
      local session = sessions[i]
      local selected = expandedSessionId == session.id
      local rowStart = ui.getCursor()
      if selected then
        ui.drawRectFilled(rowStart, rowStart + vec2(3, 32), colors.purple, 2)
      end
      local label = string.format('%s###%s', sessionTitle(session), session.id)
      if ui.selectable(label, selected, ui.SelectableFlags.None, vec2(0, 32)) then
        expandedSessionId = session.id
        pendingSelection = nil
      end
    end
  end)

  if not selectedSession then return end
  ui.dummy(10)
  local summaryStart = ui.getCursor()
  local summaryWidth = ui.availableSpaceX()
  ui.drawRectFilled(summaryStart, summaryStart + vec2(summaryWidth, 48), colors.raised, 4)
  ui.setCursor(summaryStart + vec2(10, 7))
  ui.dwriteText(sessionTitle(selectedSession), 12, colors.text)
  ui.setCursor(summaryStart + vec2(10, 26))
  ui.dwriteText(sessionDetail(selectedSession), 10, colors.muted)
  ui.setCursor(summaryStart + vec2(0, 56))
  ui.dwriteText('LAPS', 10, colors.muted)
  if not selectedSession.exactMatch then
    ui.sameLine()
    ui.dwriteText('DIFFERENT TRACK', 10, colors.red)
  end
  ui.childWindow('matchingLaps', vec2(0, ui.availableSpaceY()), false, function()
    for lapIndex = 1, #(selectedSession.laps or {}) do
      local lap = selectedSession.laps[lapIndex]
      if ui.selectable(lapLabel(selectedSession, lap), false, ui.SelectableFlags.None, vec2(0, 28)) then
        if selectedSession.exactMatch then
          activateSession(selectedSession, lap)
        else
          pendingSelection = { session = selectedSession, lap = lap }
        end
      end
    end
    if pendingSelection and pendingSelection.session.id == selectedSession.id then
      ui.dummy(6)
      ui.dwriteText('This layout might not align with the reference cues.', 11, colors.red)
      if ui.button('CANCEL##trackOverride', vec2(90, 26)) then
        pendingSelection = nil
      end
      ui.sameLine()
      if ui.button('LOAD ANYWAY##trackOverride', vec2(120, 26)) then
        activateSession(pendingSelection.session, pendingSelection.lap)
      end
    end
  end)
end

local function ensureProfileLoaded()
  if profileLoaded then return end
  profileLoaded = true
  loadProfile()
end

local function ensureSessionsRequested()
  if sessionsRequested then return end
  sessionsRequested = true
  refreshSessions()
end

local function openSettings()
  ac.setWindowOpen('main', true)
end

local function coachState()
  if not profile then
    return nil, profileError or 'No reference loaded.'
  end

  local car = ac.getCar(0)
  if not car then
    return nil, 'Waiting for the player car.'
  end
  local wrongTrack = ac.getTrackID() ~= profile.trackId or ac.getTrackLayout() ~= (profile.layoutId or '')
  if (wrongTrack and not profileTrackOverride) or ac.getCarID(0) ~= profile.carId then
    return nil, 'Reference does not match this car and track.'
  end

  local distanceM = math.saturate(car.splinePosition or 0) * profile.trackLengthM
  local target = sampleAt(distanceM)
  local zone = activeOrNextZone(distanceM)
  local secondsToBrake = nil
  if zone and target and target.e then
    local zoneStart = sampleAt(zone.startM)
    if zoneStart and zoneStart.e then
      secondsToBrake = zoneStart.e - target.e
      if secondsToBrake < 0 then
        local duration = referenceLapDuration()
        if duration then secondsToBrake = secondsToBrake + duration end
      end
    end
  end

  return {
    car = car,
    target = target,
    zone = zone,
    distanceM = distanceM,
    secondsToBrake = secondsToBrake,
    wrongTrack = wrongTrack
  }, nil
end

local function drawUnavailable(message)
  ui.setCursor(vec2(hudPadding, hudPadding))
  ui.dwriteText(message, 12, colors.muted)
  ui.dummy(5)
  if ui.button('OPEN SETTINGS', vec2(108, 25)) then openSettings() end
end

local function drawFlatSurface(surface)
  ui.drawRectFilled(vec2(0, 0), ui.windowSize(), surface or colors.surface, 0)
end

local function centeredText(text, size, y, color, height)
  ui.setCursor(vec2(hudPadding, y))
  ui.dwriteTextAligned(text, size, ui.Alignment.Center, ui.Alignment.Center, vec2(ui.windowWidth() - hudPadding * 2, height or size + 8), false, color)
end

local function countdownLabel(state)
  if state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM then return 'BRAKE' end
  if state.secondsToBrake then return tostring(math.max(1, math.ceil(state.secondsToBrake))) end
  return '—'
end

function script.windowBrake()
  ensureProfileLoaded()
  local state, stateError = coachState()
  if not state then
    drawFlatSurface(colors.surface)
    drawUnavailable(stateError)
    return
  end
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  if not isBraking and (not state.secondsToBrake or state.secondsToBrake > 3) then return end

  local urgency = isBraking and 1 or math.saturate((3 - state.secondsToBrake) / 3)
  local surface = rgbm(0.16 + 0.62 * urgency, 0.055, 0.055, 0.98)
  drawFlatSurface(surface)
  centeredText(countdownLabel(state), isBraking and 44 or 68, 0, colors.text, ui.windowHeight())
end

local function drawBrakeTraceSeries(state, graphStart, graphSize, source, color, thickness)
  local points = 100
  local previous = nil
  for index = 0, points do
    local fraction = index / points
    local relativeM = -brakeTraceBehindMetres + (brakeTraceBehindMetres + brakeTraceAheadMetres) * fraction
    local distanceM = (state.distanceM + relativeM) % profile.trackLengthM
    local value = source(distanceM)
    if value ~= nil then
      local point = vec2(
        graphStart.x + graphSize.x * fraction,
        graphStart.y + graphSize.y * (1 - math.saturate(value / 100))
      )
      if previous then ui.drawLine(previous, point, color, thickness) end
      previous = point
    else
      previous = nil
    end
  end
end

function script.windowBrakeComparison()
  ensureProfileLoaded()
  local state, stateError = coachState()
  if not state then
    drawFlatSurface(colors.surface)
    drawUnavailable(stateError)
    return
  end
  recordLiveBrake(state)
  drawFlatSurface(colors.surface)

  local liveBrake = liveBrakePercent(state.car.brake)
  local referenceBrake = math.clamp(state.target and state.target.b or 0, 0, 100)
  local halfWidth = (ui.windowWidth() - hudPadding * 2) / 2
  ui.setCursor(vec2(hudPadding, 8))
  ui.dwriteTextAligned('YOU', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(halfWidth, 12), false, colors.red)
  ui.setCursor(vec2(hudPadding, 20))
  ui.dwriteTextAligned(string.format('%d%%', math.floor(liveBrake + 0.5)), 24, ui.Alignment.Center, ui.Alignment.Center, vec2(halfWidth, 28), false, colors.text)
  ui.setCursor(vec2(hudPadding + halfWidth, 8))
  ui.dwriteTextAligned('REFERENCE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(halfWidth, 12), false, colors.purple)
  ui.setCursor(vec2(hudPadding + halfWidth, 20))
  ui.dwriteTextAligned(string.format('%d%%', math.floor(referenceBrake + 0.5)), 24, ui.Alignment.Center, ui.Alignment.Center, vec2(halfWidth, 28), false, colors.text)

  local graphStart = vec2(hudPadding + 30, 58)
  local graphSize = vec2(ui.windowWidth() - graphStart.x - hudPadding, ui.windowHeight() - graphStart.y - 14)
  ui.drawRectFilled(graphStart, graphStart + graphSize, rgbm(0.055, 0.055, 0.055, 1), 2)
  ui.drawLine(graphStart + vec2(0, graphSize.y / 2), graphStart + vec2(graphSize.x, graphSize.y / 2), colors.track, 1)

  ui.setCursor(vec2(hudPadding, graphStart.y - 6))
  ui.dwriteText('100', 9, colors.muted)
  ui.setCursor(vec2(hudPadding + 8, graphStart.y + graphSize.y - 8))
  ui.dwriteText('0', 9, colors.muted)

  drawBrakeTraceSeries(state, graphStart, graphSize, function(distanceM)
    local target = sampleAt(distanceM)
    return target and target.b or nil
  end, colors.purple, 2)
  drawBrakeTraceSeries(state, graphStart, graphSize, liveBrakeAt, colors.red, 2.5)

  local cursorFraction = brakeTraceBehindMetres / (brakeTraceBehindMetres + brakeTraceAheadMetres)
  local cursorX = graphStart.x + graphSize.x * cursorFraction
  ui.drawLine(vec2(cursorX, graphStart.y - 3), vec2(cursorX, graphStart.y + graphSize.y + 3), colors.text, 1)
end

local function drawHudSettings()
  ui.dwriteText('DRIVING HUDS', 13, colors.text)
  ui.dwriteText('Open and position either borderless HUD with CSP. The countdown appears only for 3, 2, 1 and BRAKE.', 11, colors.muted)
  ui.dummy(9)
  local buttonWidth = math.max(120, (ui.availableSpaceX() - 8) / 2)
  if ui.button('BRAKE COUNTDOWN', vec2(buttonWidth, 34)) then ac.setWindowOpen('brake', true) end
  ui.sameLine()
  if ui.button('BRAKE COMPARISON', vec2(buttonWidth, 34)) then ac.setWindowOpen('comparison', true) end
end

local function drawSettingsNavigation()
  local gap = 8
  local width = (ui.availableSpaceX() - gap) / 2
  local referenceLabel = settingsPage == 'reference' and 'REFERENCE  ●' or 'REFERENCE'
  local hudLabel = settingsPage == 'huds' and 'HUDS  ●' or 'HUDS'
  if ui.button(referenceLabel .. '##settingsReference', vec2(width, 32)) then settingsPage = 'reference' end
  ui.sameLine()
  if ui.button(hudLabel .. '##settingsHuds', vec2(width, 32)) then settingsPage = 'huds' end
end

function script.windowSettings()
  ensureProfileLoaded()
  ensureSessionsRequested()
  drawSettingsNavigation()
  ui.dummy(12)
  if settingsPage == 'reference' then
    drawSessionPicker()
  else
    drawHudSettings()
  end
end
