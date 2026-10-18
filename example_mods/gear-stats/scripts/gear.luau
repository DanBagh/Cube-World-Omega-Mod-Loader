-- Gear Stats: a menu that adds up the stats of the gear you have on. Pointing at a
-- stat shows a popup with the items it comes from, the most first, each name in its
-- rarity's color as the game's tooltips have it.

local menu = cwo.menu("stats")
local popup = menu:popup("items")

-- in the order the game's tooltips list them, each in a color of its own
local STATS = {
	{ stat = "attack", color = { 250, 120, 60 } },
	{ stat = "armor", color = { 110, 170, 250 } },
	{ stat = "health", color = { 220, 30, 40 } },
	{ stat = "resistance", color = { 190, 110, 250 } },
	{ stat = "regeneration", color = { 80, 220, 90 } },
	{ stat = "critical", color = { 250, 210, 50 } },
	{ stat = "haste", color = { 70, 210, 230 } },
}

-- spacing, in voxels
local MARGIN = 6
local GAP = 4
local TITLE_GAP = 4
local ROW_GAP = 2
local VALUE_GAP = 12
local POPUP_GAP = 10
-- item names wider than this are cut with dots, as the inventory cuts them
local NAME_WIDTH = 150
local MOST_ITEMS = 10

local layout -- where everything goes, worked out as the menu opens
local totals = {} -- each stat as the menu shows it
local shown -- what the popup shows: a row and its items

local function name(stat)
	return cwo.text("stat." .. stat.stat)
end

local function short(stat)
	return cwo.text("short." .. stat.stat)
end

local function measure()
	local names, shorts = 0, 0
	for _, stat in STATS do
		names = math.max(names, cwo.ui.width(name(stat)))
		shorts = math.max(shorts, cwo.ui.width(short(stat)))
	end
	local rows = names + GAP + shorts + VALUE_GAP + cwo.ui.width("00000")
	local first = MARGIN + cwo.ui.height("title") + TITLE_GAP
	local row = cwo.ui.height() + ROW_GAP
	return {
		width = 2 * MARGIN + math.max(rows, cwo.ui.width(cwo.text("title"), "title")),
		height = first + #STATS * row + MARGIN,
		first = first,
		row = row,
		short = MARGIN + names + GAP,
	}
end

local function sums()
	local result = {}
	for _, stat in STATS do
		result[stat.stat] = 0
	end
	for _, item in cwo.player.gear() do
		for _, stat in STATS do
			result[stat.stat] += item:stat(stat.stat)
		end
	end
	return result
end

-- the stat under the cursor, if any
local function hovered()
	local x, y = menu:cursor()
	if not x or y < layout.first then
		return nil
	end
	local row = math.floor((y - layout.first) / layout.row) + 1
	return if row <= #STATS then row else nil
end

-- each item adding to a row's stat, with how much, the most first
local function items(row)
	local stat = STATS[row].stat
	local list = {}
	for _, item in cwo.player.gear() do
		local value = item:stat(stat)
		if math.abs(value) >= 1e-5 then
			table.insert(list, { name = item.name, value = value, color = item.color })
		end
	end
	table.sort(list, function(a, b)
		return a.value > b.value
	end)
	while #list > MOST_ITEMS do
		table.remove(list)
	end
	for _, entry in list do
		entry.name = cwo.ui.fit(entry.name, NAME_WIDTH)
		entry.text = (if entry.value > 0 then "+" else "") .. cwo.number(entry.value)
	end
	return list
end

local function same(a, b)
	if a == nil or b == nil or a.row ~= b.row or #a.items ~= #b.items then
		return false
	end
	for index, item in a.items do
		local other = b.items[index]
		if item.name ~= other.name or item.text ~= other.text then
			return false
		end
	end
	return true
end

local function size_popup(content)
	local text = cwo.ui.height()
	local names, values = 0, 0
	for _, item in content.items do
		names = math.max(names, cwo.ui.width(item.name))
		values = math.max(values, cwo.ui.width(item.text))
	end
	local heading = cwo.ui.width(name(STATS[content.row]), "title")
	local width = 2 * MARGIN + math.max(heading, cwo.ui.width(cwo.text("none")), names + VALUE_GAP + values)
	local rows = math.max(#content.items, 1)
	popup:size(width, MARGIN + cwo.ui.height("title") + TITLE_GAP + rows * (text + ROW_GAP) + MARGIN)
end

menu:on("open", function()
	layout = measure()
	menu:size(layout.width, layout.height)
	totals = sums()
	shown = nil
end)

menu:on("close", function()
	popup:hide()
end)

menu:on("frame", function()
	local now = sums()
	for _, stat in STATS do
		if now[stat.stat] ~= totals[stat.stat] then
			totals = now
			menu:redraw()
			shown = nil
			break
		end
	end

	local row = hovered()
	if not row then
		popup:hide()
		return
	end
	local content = { row = row, items = items(row) }
	if not same(content, shown) then
		shown = content
		size_popup(content)
		popup:redraw()
	end
	popup:show({ side = "right", gap = POPUP_GAP })
end)

menu:on("draw", function(canvas)
	local colors = cwo.ui.colors
	canvas:panel()
	canvas:text(MARGIN, MARGIN, cwo.text("title"), { font = "title", color = colors.title })
	local y = layout.first
	for _, stat in STATS do
		local value = cwo.number(totals[stat.stat] or 0)
		canvas:text(MARGIN, y, name(stat), { color = stat.color })
		canvas:text(layout.short, y, short(stat), { color = colors.muted })
		canvas:text(canvas.width - MARGIN - cwo.ui.width(value), y, value)
		y += layout.row
	end
end)

popup:on("draw", function(canvas)
	local colors = cwo.ui.colors
	local stat = STATS[shown.row]
	canvas:panel()
	canvas:text(MARGIN, MARGIN, name(stat), { font = "title", color = stat.color })
	local y = MARGIN + cwo.ui.height("title") + TITLE_GAP
	if #shown.items == 0 then
		canvas:text(MARGIN, y, cwo.text("none"), { color = colors.muted })
	end
	for _, item in shown.items do
		canvas:text(MARGIN, y, item.name, { color = item.color or colors.text })
		canvas:text(canvas.width - MARGIN - cwo.ui.width(item.text), y, item.text, { color = stat.color })
		y += cwo.ui.height() + ROW_GAP
	end
end)
