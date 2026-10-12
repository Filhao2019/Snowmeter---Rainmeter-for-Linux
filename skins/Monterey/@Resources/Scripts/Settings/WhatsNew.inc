[Title]
Meter=String
Text=Welcome to Monterey 1.0.1
FontSize=14
FontFace=#FontFace#
FontColor=#ForegroundColor#
DynamicVariables=1
FontWeight=600
AntiAlias=1
StringAlign=Center
X=(#SidebarWidth#/2 + #WidgetCenterX#)
Y=(55+40)
ClipString=1

[SubTitle]
Meter=String
Text=Monterey widgets for Windows
FontSize=11
FontFace=#FontFace#
FontColor=#ForegroundColor#80
DynamicVariables=1
AntiAlias=1
StringAlign=Center
X=(#SidebarWidth#/2 + #WidgetCenterX#)
Y=5R
ClipString=1

[WhatsNew]
Meter=Shape
Shape=Rectangle 20,20,(#WidgetWidth# - #SidebarWidth# - 40),255,6 | Fill Color #ForegroundColor#05 | StrokeWidth 0
X=#SidebarWidth#
Y=0R
DynamicVariables=1

[Icon1]
Meter=String
Text=⛅︎
FontSize=32
FontFace=#FontFace#
FontColor=#ForegroundColor#40
DynamicVariables=1
AntiAlias=1
X=(#SidebarWidth# + 30)
Y=30r

[Title1]
Meter=String
Text=Weather widget fix
FontSize=11
FontFace=#FontFace#
FontColor=#ForegroundColor#
FontWeight=600
DynamicVariables=1
AntiAlias=1
ClipString=1
X=(#SidebarWidth# + 110)
Y=7r
W=250

[Text1]
Meter=String
Text=New forecast provider – no more API key required. Fixed city change
FontSize=10
FontWeight=400
FontFace=#FontFace#
FontColor=#ForegroundColor#80
DynamicVariables=1
AntiAlias=1
X=0r
Y=0R
W=250
H=35
ClipString=1

; [Line1]
; Meter=Shape
; Shape=Rectangle (#SidebarWidth# + 20),(250),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080

; [Icon2]
; Meter=String
; MeterStyle=Icon1
; Text=🔋
; Y=260

; [Title2]
; Meter=String
; MeterStyle=Title1
; Text=Low Power Mode
; X=(#SidebarWidth# + 110)

; [Text2]
; Meter=String
; MeterStyle=Text1
; Text=Reduces animations and refresh rate of widgets

; [Line2]
; Meter=Shape
; Shape=Rectangle (#SidebarWidth# + 20),(335),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080

; [Icon3]
; Meter=String
; MeterStyle=Icon1
; Text=🧩
; Y=345

; [Title3]
; Meter=String
; MeterStyle=Title1
; Text=Extras section
; X=(#SidebarWidth# + 110)

; [Text3]
; Meter=String
; MeterStyle=Text1
; Text=Check out new Sidebar and Unlock features

[NewUser]
Meter=String
MeterStyle=Text1
Text=New user? Check
StringAlign=Left
X=(#SidebarWidth# + 100)
Y=(#WidgetPadding# + #WidgetHeight# * 0.9)
W=105
H=(#WidgetHeight# * 0.1)

[UserGuide]
Meter=String
MeterStyle=Text1
Text=the User Guide
FontColor=#ForegroundColor#
StringAlign=Left
X=0R
Y=(#WidgetPadding# + #WidgetHeight# * 0.9)
H=(#WidgetHeight# * 0.1)
LeftMouseUpAction=["https://github.com/creewick/MontereyRainmeter/wiki/User-guide"]
W=100