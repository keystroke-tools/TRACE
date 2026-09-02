local profile = nil
local profileError = nil
local reloadTimer = 0
local configPath = ac.getFolder(ac.FolderID.ScriptConfig) .. '/reference.json'

local colors = {
  text = rgbm(0.94, 0.94, 0.94, 1),
  muted = rgbm(0.52, 0.55, 0.56, 1),
  green = rgbm(0.18, 0.86, 0.53, 1),
  red = rgbm(0.92, 0.25, 0.25, 1),
  purple = rgbm(0.70, 0.42, 1.00, 1),
  surface = rgbm(0.08, 0.08, 0.08, 0.96),
  track = rgbm(0.18, 0.18, 0.18, 1)
}

local function loadProfile()
  local encoded = io.load(configPath, '')
  if encoded == '' then
    profile = nil
    profileError = 'Choose a reference lap in TRACE.'
    return
  end
  local ok, value = pcall(JSON.parse, encoded)
  if not ok or type(value) ~= 'table' or value.schemaVersion ~= 1 then
    profile = nil
    profileError = 'The reference file is invalid or unsupported.'
    return
  end
  profile = value
  profileError = nil
end

local function drawBar(label, live, target, color)
  ui.textColored(label, colors.muted)
  ui.sameLine(64)
  local p = ui.getCursor()
  local width = math.max(100, ui.availableSpaceX())
  ui.drawRectFilled(p, p + vec2(width, 12), colors.track, 2)
  ui.drawRectFilled(p, p + vec2(width * math.saturate(live / 100), 12), color, 2)
  local markerX = p.x + width * math.saturate(target / 100)
  ui.drawLine(vec2(markerX, p.y - 2), vec2(markerX, p.y + 14), colors.text, 2)
  ui.dummy(vec2(width, 14))
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

function script.windowMain(dt)
  reloadTimer = reloadTimer - dt
  if reloadTimer <= 0 then
    reloadTimer = 1
    loadProfile()
  end

  ui.drawRectFilled(vec2(0, 0), ui.windowSize(), colors.surface)
  ui.dwriteText('TRACER //', 16, colors.green)
  ui.sameLine()

  if not profile then
    ui.newLine()
    ui.dwriteText(profileError or 'No reference loaded.', 13, colors.muted)
    return
  end

  local car = ac.getCar(0)
  if not car then
    ui.newLine()
    ui.dwriteText('Waiting for the player car.', 13, colors.muted)
    return
  end
  if ac.getTrackID() ~= profile.trackId or ac.getTrackLayout() ~= (profile.layoutId or '') or ac.getCarID(0) ~= profile.carId then
    ui.newLine()
    ui.dwriteText('Reference does not match this car and track.', 13, colors.red)
    return
  end

  local distanceM = math.saturate(car.splinePosition or 0) * profile.trackLengthM
  local target = sampleAt(distanceM)
  local zone = activeOrNextZone(distanceM)
  local distanceToBrake = zone and (zone.startM - distanceM) or nil
  if distanceToBrake and distanceToBrake < -1 and zone == profile.brakeZones[1] then
    distanceToBrake = profile.trackLengthM - distanceM + zone.startM
  end

  local cue = 'CLEAR'
  local cueColor = colors.green
  if zone and distanceM >= zone.startM and distanceM <= zone.endM then
    cue = 'BRAKE NOW'
    cueColor = colors.red
  elseif distanceToBrake and distanceToBrake <= 250 then
    cue = string.format('BRAKE IN %.0f m', math.max(0, distanceToBrake))
    cueColor = colors.purple
  end
  ui.dwriteText(cue, 22, cueColor)
  ui.dwriteText(string.format('Reference %s  |  Lap %d', profile.source.lapTime, profile.source.lapIndex), 11, colors.muted)
  ui.dummy(4)

  drawBar('BRAKE', (car.brake or 0) * 100, target and (target.b or 0) or 0, colors.red)
  drawBar('THROTTLE', (car.gas or 0) * 100, target and (target.t or 0) or 0, colors.green)
  ui.dummy(4)
  local targetGear = target and target.g or nil
  ui.dwriteText(string.format('GEAR  %s   TARGET  %s', tostring(car.gear or '-'), targetGear and tostring(targetGear) or '-'), 14, colors.text)
end
