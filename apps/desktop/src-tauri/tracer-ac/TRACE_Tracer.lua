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
    requestState = 'ready'
    requestMessage = nil
    profileTrackOverride = not session.exactMatch
    pendingSelection = nil
  end)
end

local function drawReferenceBrake(target)
  local p = ui.getCursor()
  local width = math.max(80, ui.availableSpaceX())
  local percent = math.saturate((target or 0) / 100)
  ui.drawRectFilled(p, p + vec2(width, 8), colors.track, 2)
  ui.drawRectFilled(p, p + vec2(width * percent, 8), colors.red, 2)
  ui.dummy(vec2(width, 8))
end

local function sampleAt(distanceM)
  local samples = profile.samples
  if not samples or #samples == 0 then return nil end
  local spacing = math.max(profile.sampleSpacingM or 5, 1)
  local index = math.clamp(math.floor(distanceM / spacing + 0.5) + 1, 1, #samples)
  return samples[index]
end

local function activeOrNextZone(distanceM)
  local zones = profile.brakeZones or {}
  for i = 1, #zones do
    if zones[i].endM >= distanceM then return zones[i] end
  end
  return zones[1]
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
  ui.dwriteText('CHOOSE A REFERENCE', 14, colors.text)
  ui.dwriteText(string.format('%s  ·  %s', ac.getCarName(0) or ac.getCarID(0) or 'Unknown car', ac.getTrackName() or ac.getTrackID()), 11, colors.muted)
  ui.sameLine()
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

  ui.childWindow('matchingSessions', vec2(0, ui.availableSpaceY()), false, function()
    for i = 1, #sessions do
      local session = sessions[i]
      if ui.selectable(sessionLabel(session), false, ui.SelectableFlags.None, vec2(0, 34)) then
        expandedSessionId = expandedSessionId == session.id and nil or session.id
        pendingSelection = nil
      end
      if expandedSessionId == session.id then
        if not session.exactMatch then
          ui.dwriteText('Different track: distance-aligned cues may not match this layout.', 11, colors.red)
        end
        for lapIndex = 1, #(session.laps or {}) do
          local lap = session.laps[lapIndex]
          if ui.selectable(lapLabel(session, lap), false, ui.SelectableFlags.None, vec2(0, 28)) then
            if session.exactMatch then
              activateSession(session, lap)
            else
              pendingSelection = { session = session, lap = lap }
            end
          end
        end
        if pendingSelection and pendingSelection.session.id == session.id then
          ui.dwriteText('LOAD A DIFFERENT TRACK?', 13, colors.red)
          ui.dwriteText('Only use compatible layouts. Tracer cannot guarantee cue alignment.', 11, colors.muted)
          if ui.button('CANCEL##trackOverride', vec2(90, 26)) then
            pendingSelection = nil
          end
          ui.sameLine()
          if ui.button('LOAD ANYWAY##trackOverride', vec2(120, 26)) then
            activateSession(pendingSelection.session, pendingSelection.lap)
          end
        end
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

local function openReferences()
  ac.setWindowOpen('references', true)
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
  local distanceToBrake = zone and (zone.startM - distanceM) or nil
  if distanceToBrake and distanceToBrake < 0 and zone == profile.brakeZones[1] then
    distanceToBrake = profile.trackLengthM - distanceM + zone.startM
  end

  return {
    car = car,
    target = target,
    zone = zone,
    distanceM = distanceM,
    distanceToBrake = distanceToBrake,
    wrongTrack = wrongTrack
  }, nil
end

local function drawUnavailable(message)
  ui.dwriteText(message, 12, colors.muted)
  if ui.button('REFERENCES', vec2(108, 25)) then openReferences() end
end

local function drawSurface()
  ui.drawRectFilled(vec2(0, 0), ui.windowSize(), colors.surface)
end

local function gearLabel(gear)
  if gear == nil then return '—' end
  if gear < 0 then return 'R' end
  if gear == 0 then return 'N' end
  return tostring(gear)
end

function script.windowBrake()
  ensureProfileLoaded()
  drawSurface()
  ui.dwriteText('BRAKE //', 11, colors.muted)
  local state, stateError = coachState()
  if not state then
    drawUnavailable(stateError)
    return
  end

  local cue = 'CLEAR'
  local cueColor = colors.green
  if state.zone and state.distanceM >= state.zone.startM and state.distanceM <= state.zone.endM then
    cue = 'BRAKE NOW'
    cueColor = colors.red
  elseif state.distanceToBrake and state.distanceToBrake <= 250 then
    cue = string.format('BRAKE IN %.0f m', math.max(0, state.distanceToBrake))
    cueColor = colors.purple
  end
  ui.dwriteText(cue, 24, cueColor)
  local referenceBrake = state.target and (state.target.b or 0) or 0
  ui.dwriteText(string.format('REFERENCE BRAKE  %.0f%%', referenceBrake), 10, colors.muted)
  drawReferenceBrake(referenceBrake)
  if state.wrongTrack and profileTrackOverride then
    ui.dwriteText('MANUAL TRACK OVERRIDE', 10, colors.red)
  end
end

function script.windowGear()
  ensureProfileLoaded()
  drawSurface()
  ui.dwriteText('GEAR //', 11, colors.muted)
  local state, stateError = coachState()
  if not state then
    drawUnavailable(stateError)
    return
  end
  ui.dwriteText('CURRENT', 10, colors.muted)
  ui.sameLine(74)
  ui.dwriteText('REFERENCE', 10, colors.muted)
  ui.dwriteText(gearLabel(state.car.gear), 38, colors.text)
  ui.sameLine(74)
  ui.dwriteText(gearLabel(state.target and state.target.g or nil), 38, colors.purple)
end

function script.windowReferences()
  ensureProfileLoaded()
  ensureSessionsRequested()
  drawSurface()
  ui.dwriteText('TRACER // REFERENCES', 16, colors.green)
  ui.dummy(5)
  drawSessionPicker()
end
