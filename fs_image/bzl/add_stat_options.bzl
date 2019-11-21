def add_stat_options(d, mode, user, group):
    if mode != None:
        d["mode"] = mode
    if user != None or group != None:
        if user == None:
            user = "root"
        if group == None:
            group = "root"
        d["user_group"] = "{}:{}".format(user, group)
