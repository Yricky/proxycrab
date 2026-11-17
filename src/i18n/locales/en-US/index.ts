import common from "./common";
import api from "./api";
import components from "./components";
import permissions from "./permissions";
import stores from "./stores";
import utils from "./utils";
import windows from "./windows";
import sessionPages from "./session-pages";

export default {
  ...common,
  ...api,
  ...components,
  ...permissions,
  ...stores,
  ...utils,
  ...windows,
  ...sessionPages,
};
