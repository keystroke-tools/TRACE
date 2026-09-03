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
local preferences = ac.storage({
  countdownLeadSeconds = 10
})

local colors = {
  text = rgbm(0.94, 0.94, 0.94, 1),
  muted = rgbm(0.52, 0.55, 0.56, 1),
  green = rgbm(0.18, 0.86, 0.53, 1),
  red = rgbm(0.92, 0.25, 0.25, 1),
  purple = rgbm(0.70, 0.42, 1.00, 1),
  surface = rgbm(0.08, 0.08, 0.08, 0.96),
  raised = rgbm(0.12, 0.12, 0.12, 1),
  track = rgbm(0.18, 0.18, 0.18, 1)
}
local hudInset = 6
local hudPadding = 12
local throttleCuePercent = 30
local throttleCueVisibleMetres = 60

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

local function activeThrottleCue(distanceM)
  local cues = profile and profile.throttleCues or {}
  for index = 1, #cues do
    local offset = distanceM - cues[index].startM
    if offset < 0 then offset = offset + profile.trackLengthM end
    if offset >= 0 and offset <= throttleCueVisibleMetres then return cues[index] end
  end
  return nil
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
  local throttleCue = activeThrottleCue(distanceM)
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
    throttleCue = throttleCue,
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

local function drawHudSurface(accent, surface)
  ui.drawRectFilled(vec2(hudInset, hudInset), ui.windowSize() - vec2(hudInset, hudInset), surface or colors.surface, 6)
  ui.drawRectFilled(vec2(hudInset, 16), vec2(hudInset + 3, ui.windowHeight() - 16), accent, 2)
end

local function brakeHudStyle(state)
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  if isBraking then return colors.red, rgbm(0.78, 0.10, 0.10, 0.98), true, true end
  if not state.secondsToBrake then return colors.muted, colors.surface, false, false end
  local urgency = math.saturate((5 - state.secondsToBrake) / 5)
  return rgbm(0.92, 0.25, 0.25, 1), rgbm(0.08 + 0.62 * urgency, 0.08 - 0.03 * urgency, 0.08 - 0.03 * urgency, 0.98), urgency > 0, false
end

local function centeredText(text, size, y, color, height)
  ui.setCursor(vec2(hudPadding, y))
  ui.dwriteTextAligned(text, size, ui.Alignment.Center, ui.Alignment.Center, vec2(ui.windowWidth() - hudPadding * 2, height or size + 8), false, color)
end

local function countdownLabel(state)
  if state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM then return 'BRAKE NOW' end
  if state.secondsToBrake then return tostring(math.max(1, math.ceil(state.secondsToBrake))) end
  return '—'
end

local function gearLabel(gear)
  if gear == nil then return '—' end
  if gear < 0 then return 'R' end
  if gear == 0 then return 'N' end
  return tostring(gear)
end

local function speedDeltaLabel(state)
  local referenceSpeed = state.target and state.target.s
  local liveSpeed = state.car and state.car.speedKmh
  if not referenceSpeed or not liveSpeed then return '—', colors.muted end
  local rawDelta = liveSpeed - referenceSpeed
  local rounded = rawDelta >= 0 and math.floor(rawDelta + 0.5) or math.ceil(rawDelta - 0.5)
  if rounded > 2 then return string.format('%+d KM/H', rounded), colors.red end
  if rounded < -2 then return string.format('%+d KM/H', rounded), colors.purple end
  return string.format('%+d KM/H', rounded), colors.green
end

local function throttleTarget(state)
  local target = state.target and state.target.t
  if not target then return '—', 0 end
  local percentage = math.clamp(math.floor(target + 0.5), 0, 100)
  return string.format('%d%%', percentage), percentage / 100
end

function script.windowBrake()
  ensureProfileLoaded()
  local state, stateError = coachState()
  if not state then
    drawHudSurface(colors.red)
    drawUnavailable(stateError)
    return
  end
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  if not isBraking and (not state.secondsToBrake or state.secondsToBrake > preferences.countdownLeadSeconds) then return end

  local accent, surface, urgent = brakeHudStyle(state)
  drawHudSurface(accent, surface)
  centeredText(countdownLabel(state), isBraking and 44 or 68, 14, colors.text, 84)
end

function script.windowGear()
  ensureProfileLoaded()
  drawHudSurface(colors.purple)
  local state, stateError = coachState()
  if not state then
    drawUnavailable(stateError)
    return
  end
  local gap = 12
  local contentWidth = ui.windowWidth() - hudPadding * 2
  local half = (contentWidth - gap) / 2
  local secondX = hudPadding + half + gap
  ui.drawLine(vec2(hudPadding + half + gap / 2, 18), vec2(hudPadding + half + gap / 2, ui.windowHeight() - 14), colors.track, 1)
  ui.setCursor(vec2(hudPadding, 11))
  ui.dwriteTextAligned('CURRENT', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 14), false, colors.muted)
  ui.setCursor(vec2(secondX, 11))
  ui.dwriteTextAligned('REFERENCE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 14), false, colors.purple)
  ui.setCursor(vec2(hudPadding, 30))
  ui.dwriteTextAligned(gearLabel(state.car.gear), 44, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 48), false, colors.text)
  ui.setCursor(vec2(secondX, 30))
  ui.dwriteTextAligned(gearLabel(state.target and state.target.g or nil), 44, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 48), false, colors.purple)
end

function script.windowProgress()
  ensureProfileLoaded()
  local state, stateError = coachState()
  if not state then
    drawHudSurface(colors.red)
    drawUnavailable(stateError)
    return
  end
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  if not isBraking and (not state.secondsToBrake or state.secondsToBrake > preferences.countdownLeadSeconds) then return end
  local accent, surface, urgent = brakeHudStyle(state)
  drawHudSurface(accent, surface)
  local display = isBraking and '0s' or countdownLabel(state) .. 's'
  ui.setCursor(vec2(hudPadding, 7))
  ui.dwriteTextAligned(display, 22, ui.Alignment.Center, ui.Alignment.Center, vec2(ui.windowWidth() - hudPadding * 2, 24), false, colors.text)
  local barStart = vec2(hudPadding + 4, 39)
  local barWidth = ui.windowWidth() - (hudPadding + 4) * 2
  local progress = isBraking and 1 or (state.secondsToBrake and math.saturate((5 - state.secondsToBrake) / 5) or 0)
  local centre = barStart.x + barWidth / 2
  local marker = centre + barWidth / 2 * progress
  ui.drawRectFilled(barStart, barStart + vec2(barWidth, 12), colors.track, 3)
  ui.drawRectFilled(vec2(centre, barStart.y), vec2(marker, barStart.y + 12), accent, 3)
  ui.drawLine(vec2(centre, barStart.y - 4), vec2(centre, barStart.y + 16), colors.muted, 1)
  ui.drawRectFilled(vec2(marker - 2, barStart.y - 3), vec2(marker + 2, barStart.y + 15), colors.text, 2)
end

function script.windowCoach()
  ensureProfileLoaded()
  local state, stateError = coachState()
  if not state then
    drawHudSurface(colors.red)
    drawUnavailable(stateError)
    return
  end
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  local brakeText = isBraking and 'BRAKE NOW' or (state.secondsToBrake and state.secondsToBrake <= preferences.countdownLeadSeconds and countdownLabel(state) .. 's' or '—')
  local throttleText, throttleAmount = throttleTarget(state)
  local speedText, speedColor = speedDeltaLabel(state)
  local accent = isBraking and colors.red or (state.throttleCue and colors.green or colors.purple)
  local surface = isBraking and rgbm(0.58, 0.08, 0.08, 0.98) or (state.throttleCue and rgbm(0.06, 0.26, 0.15, 0.98) or colors.surface)
  drawHudSurface(accent, surface)
  local gap = 14
  local contentWidth = ui.windowWidth() - hudPadding * 2
  local third = (contentWidth - gap * 2) / 3
  local secondX = hudPadding + third + gap
  local thirdX = secondX + third + gap
  ui.drawLine(vec2(hudPadding + third + gap / 2, 18), vec2(hudPadding + third + gap / 2, ui.windowHeight() - 18), colors.track, 1)
  ui.drawLine(vec2(secondX + third + gap / 2, 18), vec2(secondX + third + gap / 2, ui.windowHeight() - 18), colors.track, 1)
  ui.setCursor(vec2(hudPadding, 13))
  ui.dwriteTextAligned('BRAKE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(secondX, 13))
  ui.dwriteTextAligned('THROTTLE TARGET', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(thirdX, 13))
  ui.dwriteTextAligned('GEAR', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(hudPadding, 33))
  ui.dwriteTextAligned(brakeText, isBraking and 25 or 34, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 36), false, colors.text)
  ui.setCursor(vec2(secondX, 33))
  ui.dwriteTextAligned(throttleText, 34, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 36), false, state.throttleCue and colors.green or colors.text)
  ui.setCursor(vec2(thirdX, 33))
  ui.dwriteTextAligned(string.format('%s → %s', gearLabel(state.car.gear), gearLabel(state.target and state.target.g or nil)), 29, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 36), false, colors.purple)
  ui.setCursor(vec2(hudPadding, 72))
  ui.dwriteTextAligned(speedText, 14, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 18), false, speedColor)
  ui.setCursor(vec2(secondX, 72))
  ui.dwriteTextAligned(state.throttleCue and 'APPLY NOW' or 'REFERENCE TARGET', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 16), false, state.throttleCue and colors.green or colors.muted)
  local barStart = vec2(secondX, 93)
  ui.drawRectFilled(barStart, barStart + vec2(third, 8), colors.track, 3)
  ui.drawRectFilled(barStart, barStart + vec2(third * throttleAmount, 8), colors.green, 3)
end

local function drawHudSettings()
  ui.dwriteText('BRAKING CUE', 13, colors.text)
  ui.dwriteText('Show the countdown this many seconds before the reference braking point.', 11, colors.muted)
  ui.dummy(8)
  for index, seconds in ipairs({ 5, 8, 10 }) do
    if index > 1 then ui.sameLine() end
    local selected = preferences.countdownLeadSeconds == seconds
    local label = selected and string.format('%ds *##lead-%d', seconds, seconds) or string.format('%ds##lead-%d', seconds, seconds)
    if ui.button(label, vec2(52, 26)) then preferences.countdownLeadSeconds = seconds end
  end
  ui.dummy(14)
  ui.separator()
  ui.dummy(12)
  ui.dwriteText('COACHING WINDOWS', 13, colors.text)
  ui.dwriteText('Open a window, then position it with CSP. Closing it turns that view off.', 11, colors.muted)
  ui.dummy(9)
  local buttonWidth = math.max(120, (ui.availableSpaceX() - 8) / 2)
  if ui.button('BRAKE', vec2(buttonWidth, 30)) then ac.setWindowOpen('brake', true) end
  ui.sameLine()
  if ui.button('PROGRESS', vec2(buttonWidth, 30)) then ac.setWindowOpen('progress', true) end
  if ui.button('GEAR', vec2(buttonWidth, 30)) then ac.setWindowOpen('gear', true) end
  ui.sameLine()
  if ui.button('COMBINED', vec2(buttonWidth, 30)) then ac.setWindowOpen('coach', true) end
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
