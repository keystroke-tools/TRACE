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

local function activateSession(session, lap)
  local request = currentIdentity()
  request.sessionId = session.id
  request.lapIndex = lap.index
  request.allowTrackMismatch = not session.exactMatch
  requestState = 'preparing'
  requestMessage = string.format('Preparing %s…', lap.time)
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
    if value.sessionId ~= session.id or value.lapIndex ~= lap.index or source.sessionId ~= session.id or source.lapIndex ~= lap.index then
      profile = nil
      profileError = 'TRACE returned a different lap than the one selected.'
      requestState = 'error'
      requestMessage = profileError
      return
    end
    requestState = 'ready'
    requestMessage = string.format('Loaded lap %d · %s', lap.index, lap.time)
    profileTrackOverride = not session.exactMatch
    pendingSelection = nil
  end)
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
    if offset >= 0 and offset <= 30 then return cues[index] end
  end
  return nil
end

local function sessionLabel(session)
  local identity = session.driver or 'Unknown driver'
  if session.title and session.title ~= session.driver then
    identity = identity .. '  ·  ' .. session.title
  end
  local date = (session.startedAt or ''):gsub('T', ' '):sub(1, 16)
  local sessionType = string.upper(session.sessionType or 'session')
  local best = session.bestLapTime or 'no valid lap'
  local track = session.track or 'Unknown track'
  if session.layoutId and session.layoutId ~= '' then track = track .. ' / ' .. session.layoutId end
  return string.format('%s  ·  %s  ·  %s  ·  %s  ·  %s###%s', identity, best, sessionType, track, date, session.id)
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
    ui.drawRectFilled(start, start + vec2(width, 38), rgbm(0.18, 0.11, 0.25, 1), 4)
    ui.setCursor(start + vec2(10, 6))
    ui.dwriteText('ACTIVE REFERENCE', 10, colors.purple)
    ui.setCursor(start + vec2(10, 19))
    ui.dwriteText(string.format('%s  ·  LAP %d  ·  %s', identity, source.lapIndex, source.lapTime), 11, colors.text)
    ui.dummy(vec2(width, 44))
  end
  ui.dwriteText('CHOOSE A REFERENCE', 13, colors.text)
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
  local sessionListHeight = selectedSession and math.max(80, math.min(availableHeight * 0.42, 190)) or availableHeight
  ui.dwriteText('SESSIONS', 10, colors.muted)
  ui.childWindow('matchingSessions', vec2(0, sessionListHeight), false, function()
    for i = 1, #sessions do
      local session = sessions[i]
      if ui.selectable(sessionLabel(session), expandedSessionId == session.id, ui.SelectableFlags.None, vec2(0, 34)) then
        expandedSessionId = session.id
        pendingSelection = nil
      end
    end
  end)

  if not selectedSession then return end
  ui.dummy(8)
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
  ui.dwriteText(message, 12, colors.muted)
  if ui.button('OPEN SETTINGS', vec2(108, 25)) then openSettings() end
end

local function drawHudSurface(accent, surface)
  ui.drawRectFilled(vec2(0, 0), ui.windowSize(), surface or colors.surface, 6)
  ui.drawRectFilled(vec2(0, 12), vec2(3, ui.windowHeight() - 12), accent, 2)
end

local function brakeHudStyle(state)
  local isBraking = state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM
  if isBraking then return colors.red, rgbm(0.78, 0.10, 0.10, 0.98), true, true end
  if not state.secondsToBrake then return colors.muted, colors.surface, false, false end
  local urgency = math.saturate((5 - state.secondsToBrake) / 5)
  return rgbm(0.92, 0.25, 0.25, 1), rgbm(0.08 + 0.62 * urgency, 0.08 - 0.03 * urgency, 0.08 - 0.03 * urgency, 0.98), urgency > 0, false
end

local function centeredText(text, size, y, color, height)
  ui.setCursor(vec2(0, y))
  ui.dwriteTextAligned(text, size, ui.Alignment.Center, ui.Alignment.Center, vec2(ui.windowWidth(), height or size + 8), false, color)
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
  centeredText(countdownLabel(state), isBraking and 46 or 72, 14, colors.text, 84)
end

function script.windowGear()
  ensureProfileLoaded()
  drawHudSurface(colors.purple)
  local state, stateError = coachState()
  if not state then
    drawUnavailable(stateError)
    return
  end
  local half = ui.windowWidth() / 2
  ui.drawLine(vec2(half, 18), vec2(half, ui.windowHeight() - 12), colors.track, 1)
  ui.setCursor(vec2(0, 10))
  ui.dwriteTextAligned('CURRENT', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 14), false, colors.muted)
  ui.setCursor(vec2(half, 10))
  ui.dwriteTextAligned('REFERENCE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 14), false, colors.purple)
  ui.setCursor(vec2(0, 28))
  ui.dwriteTextAligned(gearLabel(state.car.gear), 46, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 52), false, colors.text)
  ui.setCursor(vec2(half, 28))
  ui.dwriteTextAligned(gearLabel(state.target and state.target.g or nil), 46, ui.Alignment.Center, ui.Alignment.Center, vec2(half, 52), false, colors.purple)
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
  ui.setCursor(vec2(0, 3))
  ui.dwriteTextAligned(display, 24, ui.Alignment.Center, ui.Alignment.Center, vec2(ui.windowWidth(), 28), false, colors.text)
  local barStart = vec2(12, 40)
  local barWidth = ui.windowWidth() - 24
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
  local throttleText = state.throttleCue and 'THROTTLE NOW' or '—'
  local accent = isBraking and colors.red or (state.throttleCue and colors.green or colors.purple)
  local surface = isBraking and rgbm(0.58, 0.08, 0.08, 0.98) or (state.throttleCue and rgbm(0.06, 0.26, 0.15, 0.98) or colors.surface)
  drawHudSurface(accent, surface)
  local third = ui.windowWidth() / 3
  ui.drawLine(vec2(third, 14), vec2(third, ui.windowHeight() - 14), colors.track, 1)
  ui.drawLine(vec2(third * 2, 14), vec2(third * 2, ui.windowHeight() - 14), colors.track, 1)
  ui.setCursor(vec2(0, 10))
  ui.dwriteTextAligned('BRAKE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(third, 10))
  ui.dwriteTextAligned('THROTTLE', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(third * 2, 10))
  ui.dwriteTextAligned('GEAR', 10, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 14), false, colors.muted)
  ui.setCursor(vec2(0, 30))
  ui.dwriteTextAligned(brakeText, isBraking and 28 or 38, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 46), false, colors.text)
  ui.setCursor(vec2(third, 30))
  ui.dwriteTextAligned(throttleText, state.throttleCue and 22 or 38, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 46), false, state.throttleCue and colors.text or colors.muted)
  ui.setCursor(vec2(third * 2, 30))
  ui.dwriteTextAligned(string.format('%s → %s', gearLabel(state.car.gear), gearLabel(state.target and state.target.g or nil)), 31, ui.Alignment.Center, ui.Alignment.Center, vec2(third, 46), false, colors.purple)
end

local function drawHudSettings()
  ui.dwriteText('COUNTDOWN', 13, colors.text)
  ui.dwriteText('Choose how early the Brake and Progress HUDs appear.', 11, colors.muted)
  ui.dummy(6)
  for index, seconds in ipairs({ 5, 8, 10 }) do
    if index > 1 then ui.sameLine() end
    local selected = preferences.countdownLeadSeconds == seconds
    local label = selected and string.format('%ds *##lead-%d', seconds, seconds) or string.format('%ds##lead-%d', seconds, seconds)
    if ui.button(label, vec2(52, 26)) then preferences.countdownLeadSeconds = seconds end
  end
  ui.dummy(16)
  ui.separator()
  ui.dummy(12)
  ui.dwriteText('OPEN A HUD', 13, colors.text)
  ui.dwriteText('Each HUD is its own app window. Close the ones you do not want.', 11, colors.muted)
  ui.dummy(7)
  local buttonWidth = math.max(120, (ui.availableSpaceX() - 8) / 2)
  if ui.button('BRAKE', vec2(buttonWidth, 30)) then ac.setWindowOpen('brake', true) end
  ui.sameLine()
  if ui.button('PROGRESS', vec2(buttonWidth, 30)) then ac.setWindowOpen('progress', true) end
  if ui.button('GEAR', vec2(buttonWidth, 30)) then ac.setWindowOpen('gear', true) end
  ui.sameLine()
  if ui.button('COMBINED', vec2(buttonWidth, 30)) then ac.setWindowOpen('coach', true) end
end

function script.windowSettings()
  ensureProfileLoaded()
  ensureSessionsRequested()
  ui.dwriteText('TRACER', 17, colors.green)
  ui.dwriteText('Choose the reference, then arrange the coaching HUDs around your driving view.', 11, colors.muted)
  ui.dummy(7)
  ui.tabBar('tracerSettingsTabs', function()
    ui.tabItem('REFERENCE', drawSessionPicker)
    ui.tabItem('HUD WINDOWS', drawHudSettings)
  end)
end
