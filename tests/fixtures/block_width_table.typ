// A table scoped inside `block(width: 75%)` must size its `w:tblW` to the
// container's share of the column (3750 = 75%), and the container width is
// consumed — no `block-width` degradation.
#set page(width: 10cm, height: 10cm, margin: 1cm)

#block(width: 75%)[
  #table(columns: 2, [A], [B])
]
