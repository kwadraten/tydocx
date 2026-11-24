#set enum(numbering: n => "§" + str(n) + ".")

+ Outer one
  + Inner a
  + Inner b
+ Outer two

#enum.item(7)[Overridden]
#enum.item(auto)[After override]

#set enum(reversed: true)
+ Back a
+ Back b
+ Back c
