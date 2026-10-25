-- Synthetic WoW SavedVariables fixture, no personal data.
TradeSkillMasterDB = {
 ["_scopeKeys"] = { ["realm"] = {[1]="Test Realm"}, ["factionrealm"] = {[1]="Alliance - Test Realm"} },
 ["_hash"] = 42,
 ["_version"] = 7,
 ["r@Test Realm@internalData@csvSales"] = "itemString,price,stackSize,quantity,time,source,buyer\\ni:42,100,1,2,90,Auction,Buyer\\ni:43,200,1,1,91,Auction,Other\\ni:44,300,1,1,92,Vendor,NPC",
 ["r@Test Realm@internalData@saveTimeSales"] = "100,101",
 ["r@Test Realm@internalData@csvBuys"] = "itemString,price,stackSize,quantity,time,source\ni:42,80,1,2,80,Auction",
 ["r@Test Realm@internalData@saveTimeBuys"] = "102",
 ["r@Test Realm@internalData@csvExpired"] = "itemString,stackSize,quantity,time\\ni:42,1,1,85",
 ["r@Test Realm@internalData@saveTimeExpires"] = "103",
 ["r@Test Realm@internalData@csvCancelled"] = "itemString,stackSize,quantity,time\\ni:43,1,3,86",
 ["r@Test Realm@internalData@saveTimeCancels"] = "104",
 ["r@Test Realm@internalData@csvIncome"] = "type,amount,time\\nMail,12,87",
 ["r@Test Realm@internalData@csvExpense"] = "type,amount,time\\nRepair,13,88",
}
TSMItemInfoDB = { ["itemStrings"] = { [1] = "i:42\002i:43\002i:44" }, ["names"] = { [1] = "Test, Item\002=Formula\002Vendor Item" } }
